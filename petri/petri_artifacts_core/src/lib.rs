// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Core abstractions for declaring and resolving type-safe test artifacts in
//! `petri`.
//!
//! NOTE: this crate does not define any concrete Artifact types itself.

// #![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// exported to support the `declare_artifacts!` macro
#[doc(hidden)]
pub use paste;
#[doc(hidden)]
pub use target_lexicon;

use std::cell::RefCell;
use std::ffi::OsStr;
use std::marker::PhantomData;
use std::path::Path;

/// How an artifact can be accessed.
#[derive(Debug, Clone)]
pub enum ArtifactSource {
    /// Artifact is available as a local file.
    Local(PathBuf),
    /// Artifact is available at a remote URL (not yet downloaded).
    Remote {
        /// The URL where the artifact can be fetched.
        url: String,
    },
}

/// Whether remote artifact access is allowed for a particular requirement.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RemoteAccess {
    /// Allow the artifact to resolve to a remote URL if not available locally.
    Allow,
    /// Require a local file; fail if not available locally.
    LocalOnly,
}

/// A trait that marks a type as being the type-safe ID for a petri artifact.
///
/// This trait should never be implemented manually! It will be automatically
/// implemented on the correct type when declaring artifacts using
/// [`declare_artifacts!`](crate::declare_artifacts).
pub trait ArtifactId: 'static {
    /// A globally unique ID corresponding to this artifact.
    #[doc(hidden)]
    const GLOBAL_UNIQUE_ID: &'static str;

    /// Whether this artifact can be backed by blob disk
    #[doc(hidden)]
    const SUPPORTS_BLOB_DISK: bool;

    /// Filename to use when this artifact is being written to or resolved
    /// from the test content dir.
    const FILENAME: &'static str;

    /// Target compatible with this artifact, if target specific.
    const TARGET: Option<target_lexicon::Triple>;

    /// ...in case you decide to flaunt the trait-level docs regarding manually
    /// implementing this trait.
    #[doc(hidden)]
    fn i_know_what_im_doing_with_this_manual_impl_instead_of_using_the_declare_artifacts_macro();
}

/// Generate the subdir the artifact should be stored/found in
pub fn artifact_subdir<T: ArtifactId>() -> String {
    if let Some(target) = T::TARGET {
        target.to_string()
    } else {
        "any".to_string()
    }
}

/// Artifact info for use in the lookup table
pub struct ArtifactInfo {
    /// A globally unique ID corresponding to this artifact.
    pub global_unique_id: &'static str,
    /// Whether this artifact can be backed by blob disk.
    pub supports_blob_disk: bool,
    /// Filename to use when this artifact is being written to or resolved
    /// from the test content dir.
    pub filename: &'static str,
    /// OS flavor compatible with this artifact, if OS specific.
    pub os_flavor: Option<OsFlavor>,
    /// Architecture compatible with this artifact, if architecture specific.
    pub arch: Option<MachineArch>,
}

/// A coarse-grained label used to differentiate between different OS
/// environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(missing_docs)] // Self-describing names.
pub enum OsFlavor {
    Windows,
    Linux,
    FreeBsd,
    Uefi,
}

// impl OsFlavor {
//     fn as_str(&self) -> &'static str {
//         match self {
//             OsFlavor::Windows => "windows",
//             OsFlavor::Linux => "linux",
//             OsFlavor::FreeBsd => "freebsd",
//             OsFlavor::Uefi => "uefi",
//         }
//     }
// }

/// The machine architecture supported by the artifact or VM.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[expect(missing_docs)] // Self describing names
pub enum MachineArch {
    X86_64,
    Aarch64,
}

impl MachineArch {
    /// Returns the host's architecture.
    pub fn host() -> Self {
        // xtask-fmt allow-target-arch oneoff-petri-host-arch
        if cfg!(target_arch = "x86_64") {
            Self::X86_64
        }
        // xtask-fmt allow-target-arch oneoff-petri-host-arch
        else if cfg!(target_arch = "aarch64") {
            Self::Aarch64
        } else {
            panic!("unsupported host architecture")
        }
    }

    // fn as_str(&self) -> &'static str {
    //     match self {
    //         Self::X86_64 => "x64",
    //         Self::Aarch64 => "aarch64",
    //     }
    // }
}

/// A type-safe handle to a particular Artifact, as declared using the
/// [`declare_artifacts!`](crate::declare_artifacts) macro.
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArtifactHandle<A: ArtifactId>(PhantomData<A>);

impl<A: ArtifactId + std::fmt::Debug> std::fmt::Debug for ArtifactHandle<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.erase(), f)
    }
}

/// A resolved artifact path for artifact `A`.
pub struct ResolvedArtifact<A = ()>(Option<PathBuf>, PhantomData<A>);

impl<A> Clone for ResolvedArtifact<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1)
    }
}

impl<A> std::fmt::Debug for ResolvedArtifact<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ResolvedArtifact").field(&self.0).finish()
    }
}

impl<A> ResolvedArtifact<A> {
    /// Erases the type `A`.
    pub fn erase(self) -> ResolvedArtifact {
        ResolvedArtifact(self.0, PhantomData)
    }

    /// Gets the resolved path of the artifact.
    #[track_caller]
    pub fn get(&self) -> &Path {
        self.0
            .as_ref()
            .expect("cannot get path in requirements phase")
    }
}

impl<A> From<ResolvedArtifact<A>> for PathBuf {
    #[track_caller]
    fn from(ra: ResolvedArtifact<A>) -> PathBuf {
        ra.0.expect("cannot get path in requirements phase")
    }
}

impl<A> AsRef<Path> for ResolvedArtifact<A> {
    #[track_caller]
    fn as_ref(&self) -> &Path {
        self.get()
    }
}

impl<A> AsRef<OsStr> for ResolvedArtifact<A> {
    #[track_caller]
    fn as_ref(&self) -> &OsStr {
        self.get().as_ref()
    }
}

/// A resolve artifact path for an optional artifact `A`.
#[derive(Clone, Debug)]
pub struct ResolvedOptionalArtifact<A = ()>(OptionalArtifactState, PhantomData<A>);

#[derive(Clone, Debug)]
enum OptionalArtifactState {
    Collecting,
    Missing,
    Present(PathBuf),
}

impl<A> ResolvedOptionalArtifact<A> {
    /// Erases the type `A`.
    pub fn erase(self) -> ResolvedOptionalArtifact {
        ResolvedOptionalArtifact(self.0, PhantomData)
    }

    /// Gets the resolved path of the artifact, if it was found.
    #[track_caller]
    pub fn get(&self) -> Option<&Path> {
        match self.0 {
            OptionalArtifactState::Collecting => panic!("cannot get path in requirements phase"),
            OptionalArtifactState::Missing => None,
            OptionalArtifactState::Present(ref path) => Some(path),
        }
    }
}

/// A resolved artifact source for artifact `A`, which may be local or remote.
pub struct ResolvedArtifactSource<A = ()>(Option<ArtifactSource>, PhantomData<A>);

impl<A> Clone for ResolvedArtifactSource<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1)
    }
}

impl<A> std::fmt::Debug for ResolvedArtifactSource<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ResolvedArtifactSource")
            .field(&self.0)
            .finish()
    }
}

impl<A> ResolvedArtifactSource<A> {
    /// Erases the type `A`.
    pub fn erase(self) -> ResolvedArtifactSource {
        ResolvedArtifactSource(self.0, PhantomData)
    }

    /// Gets the resolved source of the artifact.
    #[track_caller]
    pub fn get(&self) -> &ArtifactSource {
        self.0
            .as_ref()
            .expect("cannot get source in requirements phase")
    }
}

/// An artifact resolver, used both to express requirements for artifacts and to
/// resolve them to paths.
pub struct ArtifactResolver<'a> {
    inner: ArtifactResolverInner<'a>,
    remote_policy: RemoteAccess,
}

impl<'a> ArtifactResolver<'a> {
    /// Returns the remote access policy, checking the
    /// `PETRI_REMOTE_ARTIFACTS` environment variable.
    ///
    /// Set `PETRI_REMOTE_ARTIFACTS=0` to force all artifacts to be resolved
    /// locally, even if `RemoteAccess::Allow` is specified per-call or per-test.
    pub fn set_remote_policy(&mut self, test_policy: RemoteAccess) {
        self.remote_policy = if matches!(test_policy, RemoteAccess::LocalOnly)
            || matches!(
                std::env::var("PETRI_REMOTE_ARTIFACTS").as_deref(),
                Ok("0") | Ok("false")
            ) {
            RemoteAccess::LocalOnly
        } else {
            RemoteAccess::Allow
        };
    }

    /// Returns a resolver to collect requirements; the artifact objects returned by
    /// [`require`](Self::require) will panic if used.
    pub fn collector(requirements: &'a mut TestArtifactRequirements) -> Self {
        ArtifactResolver {
            inner: ArtifactResolverInner::Collecting(RefCell::new(requirements)),
            remote_policy: RemoteAccess::LocalOnly,
        }
    }

    /// Returns a resolver to resolve artifacts.
    pub fn resolver(artifacts: &'a TestArtifacts) -> Self {
        ArtifactResolver {
            inner: ArtifactResolverInner::Resolving(artifacts),
            remote_policy: RemoteAccess::LocalOnly,
        }
    }

    /// Returns the effective remote access for a given per-call policy,
    /// respecting the resolver-wide policy.
    fn effective_remote<A: ArtifactId>(&self, per_call: RemoteAccess) -> RemoteAccess {
        if matches!(self.remote_policy, RemoteAccess::LocalOnly) || !A::SUPPORTS_BLOB_DISK {
            RemoteAccess::LocalOnly
        } else {
            per_call
        }
    }

    /// Resolve a required artifact. The artifact must be available locally.
    pub fn require<A: ArtifactId>(&self, handle: ArtifactHandle<A>) -> ResolvedArtifact<A> {
        let source = self.require_source(handle, RemoteAccess::LocalOnly);
        ResolvedArtifact(
            source.0.map(|s| match s {
                ArtifactSource::Local(p) => p,
                ArtifactSource::Remote { url } => panic!(
                    "artifact required via require() resolved to remote source `{url}`; \
                     use require_source(..., RemoteAccess::Allow) or download the artifact locally"
                ),
            }),
            PhantomData,
        )
    }

    /// Resolve an optional artifact.
    pub fn try_require<A: ArtifactId>(
        &self,
        handle: ArtifactHandle<A>,
    ) -> ResolvedOptionalArtifact<A> {
        match &self.inner {
            ArtifactResolverInner::Collecting(requirements) => {
                requirements
                    .borrow_mut()
                    .require(handle.erase(), RemoteAccess::LocalOnly, true);
                ResolvedOptionalArtifact(OptionalArtifactState::Collecting, PhantomData)
            }
            ArtifactResolverInner::Resolving(artifacts) => ResolvedOptionalArtifact(
                artifacts
                    .try_get(handle)
                    .map_or(OptionalArtifactState::Missing, |p| {
                        OptionalArtifactState::Present(p.to_owned())
                    }),
                PhantomData,
            ),
        }
    }

    /// Resolve an artifact, returning either a local path or a remote URL.
    ///
    /// The `remote` parameter controls whether a remote URL is acceptable for
    /// this particular artifact. The resolver's configured remote policy may
    /// further restrict this request and force the effective access mode to
    /// `LocalOnly`.
    pub fn require_source<A: ArtifactId>(
        &self,
        handle: ArtifactHandle<A>,
        remote: RemoteAccess,
    ) -> ResolvedArtifactSource<A> {
        let effective = self.effective_remote::<A>(remote);
        match &self.inner {
            ArtifactResolverInner::Collecting(requirements) => {
                requirements
                    .borrow_mut()
                    .require(handle.erase(), effective, false);
                ResolvedArtifactSource(None, PhantomData)
            }
            ArtifactResolverInner::Resolving(artifacts) => {
                ResolvedArtifactSource(Some(artifacts.get_source(handle).clone()), PhantomData)
            }
        }
    }
}

enum ArtifactResolverInner<'a> {
    Collecting(RefCell<&'a mut TestArtifactRequirements>),
    Resolving(&'a TestArtifacts),
}

/// A type-erased handle to a particular Artifact, with no information as to
/// what exactly the artifact is.
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ErasedArtifactHandle {
    artifact_id_str: &'static str,
}

impl ErasedArtifactHandle {
    /// used to serialize the artifact handle when querying petri for test requirements
    pub fn global_unique_id(&self) -> String {
        self.artifact_id_str.to_string()
    }
}

impl std::fmt::Debug for ErasedArtifactHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // the `declare_artifacts!` macro uses `module_path!` under-the-hood to
        // generate an artifact_id_str based on the artifact's crate + module
        // path. To avoid collisions, the mod is named `TYPE_NAME__ty`, but to
        // make it easier to parse output, we strip the `__ty`.
        write!(
            f,
            "{}",
            self.artifact_id_str
                .strip_suffix("__ty")
                .unwrap_or(self.artifact_id_str)
        )
    }
}

impl<A: ArtifactId> PartialEq<ErasedArtifactHandle> for ArtifactHandle<A> {
    fn eq(&self, other: &ErasedArtifactHandle) -> bool {
        &self.erase() == other
    }
}

impl<A: ArtifactId> PartialEq<ArtifactHandle<A>> for ErasedArtifactHandle {
    fn eq(&self, other: &ArtifactHandle<A>) -> bool {
        self == &other.erase()
    }
}

impl<A: ArtifactId> ArtifactHandle<A> {
    /// Create a new typed artifact handle. It is unlikely you will need to call
    /// this directly.
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

/// Helper trait to allow uniform handling of both typed and untyped artifact
/// handles in various contexts.
pub trait AsArtifactHandle {
    /// Return a type-erased handle to the given artifact.
    fn erase(&self) -> ErasedArtifactHandle;
}

impl AsArtifactHandle for ErasedArtifactHandle {
    fn erase(&self) -> ErasedArtifactHandle {
        *self
    }
}

impl<A: ArtifactId> AsArtifactHandle for ArtifactHandle<A> {
    fn erase(&self) -> ErasedArtifactHandle {
        ErasedArtifactHandle {
            artifact_id_str: A::GLOBAL_UNIQUE_ID,
        }
    }
}

/// Declare one or more type-safe artifacts that do not support blob disk.
#[macro_export]
macro_rules! declare_artifacts {
    (
        $(
            $(#[$doc:meta])*
            $name:ident
        ),*
        $(,)?
    ) => {
        $crate::declare_artifacts_inner!(
            $(
                $(#[$doc])*
                $name(false, "", ANY),
            )*
        );
    };
}

/// Declare one or more type-safe artifacts that do not support blob disk.
#[macro_export]
macro_rules! declare_artifacts_with_filename_and_target {
    (
        $(
            $(#[$doc:meta])*
            $name:ident($filename:literal, $target:ident)
        ),*
        $(,)?
    ) => {
        $crate::declare_artifacts_inner!(
            $(
                $(#[$doc])*
                $name(false, $filename, $target),
            )*
        );
    };
}

/// Declare one or more type-safe artifacts that support blob disk.
#[macro_export]
macro_rules! declare_blob_artifacts {
    (
        $(
            $(#[$doc:meta])*
            $name:ident
        ),*
        $(,)?
    ) => {
        $crate::declare_artifacts_inner!(
            $(
                $(#[$doc])*
                $name(true, "", ANY),
            )*
        );
    };
}

#[doc(hidden)]
pub mod artifacts_macro_support {
    // UNSAFETY: Needed for linkme.
    #![expect(unsafe_code)]

    use crate::ArtifactInfo;
    pub use linkme;

    #[linkme::distributed_slice]
    pub static ARTIFACTS: [ArtifactInfo];

    // Always have at least one entry to work around linker bugs.
    //
    // See <https://github.com/llvm/llvm-project/issues/65855>.
    #[linkme::distributed_slice(ARTIFACTS)]
    static WORKAROUND: ArtifactInfo = ArtifactInfo {
        global_unique_id: "",
        supports_blob_disk: false,
        filename: "",
        os_flavor: None,
        arch: None,
    };
}

/// Get info about an artifact from its global unique ID
pub fn get_artifact_info_by_id(global_unique_id: &str) -> Option<&ArtifactInfo> {
    artifacts_macro_support::ARTIFACTS
        .iter()
        .find(|i| i.global_unique_id == global_unique_id)
}

/// Declare one or more type-safe artifacts, specifying whether each supports
/// blob disk.
#[macro_export]
macro_rules! declare_artifacts_inner {
    (
        $(
            $(#[$doc:meta])*
            $name:ident($supports_blob_disk:literal, $filename:literal, $target:ident)
        ),*
        $(,)?
    ) => {
        $(
            $crate::paste::paste! {
                $(#[$doc])*
                #[expect(non_camel_case_types)]
                pub const $name: $crate::ArtifactHandle<$name> = $crate::ArtifactHandle::new();

                #[doc = concat!("Type-tag for [`",  stringify!($name), "`]")]
                #[expect(non_camel_case_types)]
                #[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
                pub enum $name {}

                #[expect(non_snake_case)]
                mod [< $name __ty >] {
                    impl $crate::ArtifactId for super::$name {
                        const GLOBAL_UNIQUE_ID: &'static str = module_path!();
                        const SUPPORTS_BLOB_DISK: bool = $supports_blob_disk;
                        const FILENAME: &'static str = $filename;
                        const TARGET: Option<$crate::target_lexicon::Triple> = $crate::targets::$target;
                        fn i_know_what_im_doing_with_this_manual_impl_instead_of_using_the_declare_artifacts_macro() {}
                    }

                    const _: () = {
                        use $crate::artifacts_macro_support::linkme;
                        #[linkme::distributed_slice($crate::artifacts_macro_support::ARTIFACTS)]
                        #[linkme(crate = linkme)]
                        static ARTIFACT: $crate::ArtifactInfo = $crate::ArtifactInfo {
                            global_unique_id: module_path!(),
                            supports_blob_disk: $supports_blob_disk,
                            filename: $filename,
                            os_flavor: None,
                            arch: None,
                        };
                    };
                }
            }
        )*
    };
}

/// A trait to resolve artifacts to paths.
///
/// Test authors are expected to use the [`TestArtifactRequirements`] and
/// [`TestArtifacts`] abstractions to interact with artifacts, and should not
/// use this API directly.
pub trait ResolveTestArtifact {
    /// Given an artifact handle, return its corresponding PathBuf.
    ///
    /// This method must use type-erased handles, as using typed artifact
    /// handles in this API would cause the trait to no longer be object-safe.
    fn resolve(&self, id: ErasedArtifactHandle) -> anyhow::Result<PathBuf>;

    /// Given an artifact handle, return its source (local path or remote URL).
    ///
    /// The default implementation wraps the result of [`resolve`](Self::resolve)
    /// in [`ArtifactSource::Local`]. Override this to return
    /// [`ArtifactSource::Remote`] for artifacts that are available at a URL
    /// but not downloaded locally.
    fn resolve_source(&self, id: ErasedArtifactHandle) -> anyhow::Result<ArtifactSource> {
        self.resolve(id).map(ArtifactSource::Local)
    }
}

impl<T: ResolveTestArtifact + ?Sized> ResolveTestArtifact for &T {
    fn resolve(&self, id: ErasedArtifactHandle) -> anyhow::Result<PathBuf> {
        (**self).resolve(id)
    }

    fn resolve_source(&self, id: ErasedArtifactHandle) -> anyhow::Result<ArtifactSource> {
        (**self).resolve_source(id)
    }
}

/// How an artifact was required.
#[derive(Debug, Copy, Clone)]
struct ArtifactRequirement {
    optional: bool,
    remote: RemoteAccess,
}

/// A set of dependencies required to run a test.
#[derive(Clone)]
pub struct TestArtifactRequirements {
    artifacts: Vec<(ErasedArtifactHandle, ArtifactRequirement)>,
}

impl TestArtifactRequirements {
    /// Create an empty set of dependencies.
    pub fn new() -> Self {
        TestArtifactRequirements {
            artifacts: Vec::new(),
        }
    }

    /// Add a dependency that may be optional or resolve to a remote URL.
    pub fn require(
        &mut self,
        dependency: impl AsArtifactHandle,
        remote: RemoteAccess,
        optional: bool,
    ) -> &mut Self {
        self.artifacts
            .push((dependency.erase(), ArtifactRequirement { optional, remote }));
        self
    }

    /// Returns the current list of required depencencies.
    pub fn required_artifacts(&self) -> impl Iterator<Item = ErasedArtifactHandle> + '_ {
        self.artifacts
            .iter()
            .filter_map(|&(a, req)| (!req.optional).then_some(a))
    }

    /// Returns the current list of optional dependencies.
    pub fn optional_artifacts(&self) -> impl Iterator<Item = ErasedArtifactHandle> + '_ {
        self.artifacts
            .iter()
            .filter_map(|&(a, req)| req.optional.then_some(a))
    }

    /// Resolve the set of dependencies.
    ///
    /// Remote access for each artifact is determined by the
    /// [`RemoteAccess`] flags recorded during collection, subject to any
    /// process-wide override configured via the `PETRI_REMOTE_ARTIFACTS`
    /// environment variable.
    pub fn resolve(&self, resolver: impl ResolveTestArtifact) -> anyhow::Result<TestArtifacts> {
        let mut failed = String::new();
        let mut resolved = HashMap::new();

        // Merge duplicate registrations by handle, keeping the strictest
        // requirement (treat as required if any registration is required,
        // use the most restrictive remote access).
        let mut merged: HashMap<ErasedArtifactHandle, ArtifactRequirement> = HashMap::new();
        for &(a, req) in &self.artifacts {
            merged
                .entry(a)
                .and_modify(|existing| {
                    // required if any registration is required
                    existing.optional = existing.optional && req.optional;
                    // use LocalOnly if any registration requires it
                    if matches!(req.remote, RemoteAccess::LocalOnly) {
                        existing.remote = RemoteAccess::LocalOnly;
                    }
                })
                .or_insert(req);
        }

        for (a, req) in merged {
            let use_source = matches!(req.remote, RemoteAccess::Allow);

            let result = if use_source {
                resolver.resolve_source(a)
            } else {
                resolver.resolve(a).map(ArtifactSource::Local)
            };

            match result {
                Ok(source) => {
                    resolved.insert(a, source);
                }
                Err(_) if req.optional => {}
                Err(e) => failed.push_str(&format!("{:?} - {:#}\n", a, e)),
            }
        }

        if !failed.is_empty() {
            anyhow::bail!("Artifact resolution failed:\n{}", failed);
        }

        Ok(TestArtifacts {
            artifacts: Arc::new(resolved),
        })
    }
}

/// A resolved set of test artifacts, returned by
/// [`TestArtifactRequirements::resolve`].
#[derive(Clone)]
pub struct TestArtifacts {
    artifacts: Arc<HashMap<ErasedArtifactHandle, ArtifactSource>>,
}

impl TestArtifacts {
    /// Try to get the resolved path of an artifact.
    ///
    /// Returns `None` if the artifact was not required. Panics if the artifact
    /// is only available remotely.
    #[track_caller]
    pub fn try_get(&self, artifact: impl AsArtifactHandle) -> Option<&Path> {
        self.artifacts.get(&artifact.erase()).map(|source| {
            match source {
                ArtifactSource::Local(p) => p.as_path(),
                ArtifactSource::Remote { .. } => panic!(
                    "Artifact {:?} is only available remotely; use require_source() or download it locally",
                    artifact.erase()
                ),
            }
        })
    }

    /// Get the resolved path of an artifact.
    ///
    /// Panics if the artifact was not required or is only available remotely.
    #[track_caller]
    pub fn get(&self, artifact: impl AsArtifactHandle) -> &Path {
        self.try_get(artifact.erase())
            .unwrap_or_else(|| panic!("Artifact not initially required: {:?}", artifact.erase()))
    }

    /// Try to get the resolved source of an artifact.
    #[track_caller]
    pub fn try_get_source(&self, artifact: impl AsArtifactHandle) -> Option<&ArtifactSource> {
        self.artifacts.get(&artifact.erase())
    }

    /// Get the resolved source of an artifact.
    #[track_caller]
    pub fn get_source(&self, artifact: impl AsArtifactHandle) -> &ArtifactSource {
        self.try_get_source(artifact.erase())
            .unwrap_or_else(|| panic!("Artifact not initially required: {:?}", artifact.erase()))
    }
}

/// JSON output format for `--list-required-artifacts`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ArtifactListOutput {
    /// List of unique required artifact IDs across all matching tests.
    pub required: Vec<String>,
    /// List of unique optional artifact IDs across all matching tests.
    pub optional: Vec<String>,
}

/// Targets for the artifacts
pub mod targets {
    /// Artifact can be used on any target system
    pub const ANY: Option<target_lexicon::Triple> = None;
    /// x86_64-pc-windows-msvc
    pub const WINDOWS_X64: Option<target_lexicon::Triple> = Some(target_lexicon::Triple {
        architecture: target_lexicon::Architecture::X86_64,
        vendor: target_lexicon::Vendor::Pc,
        operating_system: target_lexicon::OperatingSystem::Windows,
        environment: target_lexicon::Environment::Msvc,
        binary_format: target_lexicon::BinaryFormat::Unknown,
    });
    /// x86_64-unknown-linux-gnu
    pub const LINUX_X64: Option<target_lexicon::Triple> = Some(target_lexicon::Triple {
        architecture: target_lexicon::Architecture::X86_64,
        vendor: target_lexicon::Vendor::Unknown,
        operating_system: target_lexicon::OperatingSystem::Linux,
        environment: target_lexicon::Environment::Gnu,
        binary_format: target_lexicon::BinaryFormat::Elf,
    });
    /// x86_64-unknown-linux-musl
    pub const LINUX_X64_MUSL: Option<target_lexicon::Triple> = Some(target_lexicon::Triple {
        architecture: target_lexicon::Architecture::X86_64,
        vendor: target_lexicon::Vendor::Unknown,
        operating_system: target_lexicon::OperatingSystem::Linux,
        environment: target_lexicon::Environment::Musl,
        binary_format: target_lexicon::BinaryFormat::Elf,
    });
    /// aarch64-pc-windows-msvc
    pub const WINDOWS_AARCH64: Option<target_lexicon::Triple> = Some(target_lexicon::Triple {
        architecture: target_lexicon::Architecture::Aarch64(
            target_lexicon::Aarch64Architecture::Aarch64,
        ),
        vendor: target_lexicon::Vendor::Pc,
        operating_system: target_lexicon::OperatingSystem::Windows,
        environment: target_lexicon::Environment::Msvc,
        binary_format: target_lexicon::BinaryFormat::Unknown,
    });
    /// aarch64-unknown-linux-gnu
    pub const LINUX_AARCH64: Option<target_lexicon::Triple> = Some(target_lexicon::Triple {
        architecture: target_lexicon::Architecture::Aarch64(
            target_lexicon::Aarch64Architecture::Aarch64,
        ),
        vendor: target_lexicon::Vendor::Unknown,
        operating_system: target_lexicon::OperatingSystem::Linux,
        environment: target_lexicon::Environment::Gnu,
        binary_format: target_lexicon::BinaryFormat::Elf,
    });
    /// aarch64-unknown-linux-musl
    pub const LINUX_AARCH64_MUSL: Option<target_lexicon::Triple> = Some(target_lexicon::Triple {
        architecture: target_lexicon::Architecture::Aarch64(
            target_lexicon::Aarch64Architecture::Aarch64,
        ),
        vendor: target_lexicon::Vendor::Unknown,
        operating_system: target_lexicon::OperatingSystem::Linux,
        environment: target_lexicon::Environment::Musl,
        binary_format: target_lexicon::BinaryFormat::Elf,
    });
}
