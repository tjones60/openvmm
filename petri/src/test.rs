// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Infrastructure for defining tests.

#[doc(hidden)]
pub mod test_macro_support {
    use super::RunTest;
    pub use linkme;

    #[linkme::distributed_slice]
    pub static TESTS: [fn() -> (&'static str, Vec<Box<dyn RunTest>>)] = [..];
}

use crate::TestArtifactRequirements;
use crate::TestArtifacts;
use test_macro_support::TESTS;

/// Defines a single test from a value that implements [`RunTest`].
#[macro_export]
macro_rules! test {
    ($f:ident, $req:expr) => {
        $crate::multitest!(vec![Box::new($crate::SimpleTest::new(
            stringify!($f),
            $req,
            $f
        ))]);
    };
}

/// Defines a set of tests from a [`Vec<Box<dyn RunTest>>`].
#[macro_export]
macro_rules! multitest {
    ($tests:expr) => {
        const _: () = {
            use $crate::test_macro_support::linkme;
            // UNSAFETY: linkme uses manual link sections, which are unsafe.
            #[expect(unsafe_code)]
            #[linkme::distributed_slice($crate::test_macro_support::TESTS)]
            #[linkme(crate = linkme)]
            static TEST: fn() -> (&'static str, Vec<Box<dyn $crate::RunTest>>) =
                || (module_path!(), $tests);
        };
    };
}

/// A single test.
struct Test {
    module: &'static str,
    test: Box<dyn RunTest>,
}

impl Test {
    /// Returns all the tests defined in this crate.
    fn all() -> impl Iterator<Item = Self> {
        TESTS.iter().flat_map(|f| {
            let (module, tests) = f();
            tests.into_iter().map(move |test| Self { module, test })
        })
    }

    /// Returns the name of the test.
    fn name(&self) -> String {
        // Strip the crate name from the module path, for consistency with libtest.
        let module = self
            .module
            .split_once("::")
            .map_or(self.module, |(_, rest)| rest);
        format!("{}::{}", module, self.test.leaf_name())
    }

    /// Returns the artifact requirements for the test.
    fn requirements(&self) -> TestArtifactRequirements {
        // All tests require the log directory.
        self.test
            .requirements()
            .require(petri_artifacts_common::artifacts::TEST_LOG_DIRECTORY)
    }

    /// Returns a libtest-mimic trial to run the test.
    fn trial(
        self,
        resolve: fn(&str, TestArtifactRequirements) -> anyhow::Result<TestArtifacts>,
    ) -> libtest_mimic::Trial {
        libtest_mimic::Trial::test(self.name(), move || {
            let name = self.name();
            let artifacts = resolve(&name, self.requirements())
                .map_err(|err| format!("failed to resolve artifacts: {:#}", err))?;
            self.test.run(&name, &artifacts)
        })
    }
}

/// A test that can be run.
///
/// Register it to be run with [`test!`] or [`multitest!`].
pub trait RunTest: Send {
    /// The leaf name of the test.
    ///
    /// To produce the full test name, this will be prefixed with the module
    /// name where the test is defined.
    fn leaf_name(&self) -> &str;
    /// Returns the artifacts required by the test.
    fn requirements(&self) -> TestArtifactRequirements;
    /// Runs the test, which has been assigned `name`, with the given
    /// `artifacts`.
    fn run(&self, name: &str, artifacts: &TestArtifacts) -> Result<(), libtest_mimic::Failed>;
}

/// A test defined by a fixed set of requirements and a run function.
pub struct SimpleTest<F> {
    leaf_name: &'static str,
    requirements: TestArtifactRequirements,
    run: F,
}

impl<F, E> SimpleTest<F>
where
    F: 'static + Send + Fn(&str, &TestArtifacts) -> Result<(), E>,
    E: Into<anyhow::Error>,
{
    /// Returns a new test with the given `leaf_name`, `requirements`, and `run`
    /// functions.
    pub fn new(leaf_name: &'static str, requirements: TestArtifactRequirements, run: F) -> Self {
        SimpleTest {
            leaf_name,
            requirements,
            run,
        }
    }
}

impl<F, E> RunTest for SimpleTest<F>
where
    F: 'static + Send + Fn(&str, &TestArtifacts) -> Result<(), E>,
    E: Into<anyhow::Error>,
{
    fn leaf_name(&self) -> &str {
        self.leaf_name
    }

    fn requirements(&self) -> TestArtifactRequirements {
        self.requirements.clone()
    }

    fn run(&self, name: &str, artifacts: &TestArtifacts) -> Result<(), libtest_mimic::Failed> {
        (self.run)(name, artifacts).map_err(|err| format!("{:#}", err.into()))?;
        Ok(())
    }
}

#[derive(clap::Parser)]
struct Options {
    /// Lists the required artifacts for all tests.
    #[clap(long)]
    list_required_artifacts: bool,
    #[clap(flatten)]
    inner: libtest_mimic::Arguments,
}

/// Entry point for test binaries.
pub fn test_main(
    resolve: fn(&str, TestArtifactRequirements) -> anyhow::Result<TestArtifacts>,
) -> ! {
    let mut args = <Options as clap::Parser>::parse();
    if args.list_required_artifacts {
        // FUTURE: write this in a machine readable format.
        for test in Test::all() {
            let requirements = test.requirements();
            println!("{}:", test.name());
            for artifact in requirements.required_artifacts() {
                println!("required: {artifact:?}");
            }
            for artifact in requirements.optional_artifacts() {
                println!("optional: {artifact:?}");
            }
            println!();
        }
        std::process::exit(0);
    }

    // Always just use one thread to avoid interleaving logs and to avoid using
    // too many resources. These tests are usually run under nextest, which will
    // run them in parallel in separate processes with appropriate concurrency
    // limits.
    if !matches!(args.inner.test_threads, None | Some(1)) {
        eprintln!("warning: ignoring value passed to --test-threads, using 1");
    }
    args.inner.test_threads = Some(1);

    let trials = Test::all().map(|test| test.trial(resolve)).collect();
    libtest_mimic::run(&args.inner, trials).exit()
}
