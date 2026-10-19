// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Common CVM code for the MSHV backend.

use crate::Error;
use crate::ErrorInner;
use crate::MshvPartition;
use crate::MshvPartitionInner;

#[derive(Debug, Copy, Clone, Eq, PartialEq, inspect::Inspect)]
pub(crate) enum CvmLaunchState {
    NotStarted,
    Started,
    Finished,
    Failed,
}

impl virt::AcceptInitialPages for MshvPartition {
    type Error = Error;

    fn accept_initial_pages(&self, pages: &[virt::InitialPageImport]) -> Result<(), Self::Error> {
        self.inner.cvm_launch_initial_pages(pages)
    }
}

impl MshvPartitionInner {
    fn cvm_launch_initial_pages(&self, pages: &[virt::InitialPageImport]) -> Result<(), Error> {
        let Some(snp) = self.isolation.snp() else {
            return Err(ErrorInner::IsolationNotSupported.into());
        };
        {
            let mut state = snp.launch_state.lock();
            match *state {
                SnpLaunchState::NotStarted => *state = SnpLaunchState::Started,
                SnpLaunchState::Started => return Err(SnpError::LaunchInProgress.into()),
                SnpLaunchState::Finished => return Ok(()),
                SnpLaunchState::Failed => return Err(SnpError::LaunchFailed.into()),
            }
        }

        match self.snp_launch_initial_pages_inner(snp, pages) {
            Ok(()) => {
                *snp.launch_state.lock() = SnpLaunchState::Finished;
                Ok(())
            }
            Err(err) => {
                *snp.launch_state.lock() = SnpLaunchState::Failed;
                Err(err)
            }
        }
    }

    fn snp_launch_initial_pages_inner(
        &self,
        snp: &SnpPartitionState,
        pages: &[virt::InitialPageImport],
    ) -> Result<(), Error> {
        let (vmsa_gpa, cpuid_gpa) = snp_launch_pages(pages)?;
        let sev_features = if let Some(config) = &snp.config {
            if vmsa_gpa != config.vmsa_gpa {
                return Err(ErrorInner::InvalidSnpVmsaImport.into());
            }
            config.sev_features
        } else {
            let vmsa = self
                .gm
                .read_plain::<x86defs::snp::SevVmsa>(vmsa_gpa)
                .map_err(SnpError::GuestMemory)?;
            vmsa.sev_features.into_bits()
        };
        // GHCB SNP AP-creation requests supply a new VMSA. Save the BSP launch
        // features so handle_snp_ap_create can require each AP VMSA to use the
        // same guest-visible SEV feature set.
        *snp.sev_features.lock() = Some(sev_features);
        self.write_snp_cpuid_page(cpuid_gpa)?;

        self.vmfd
            .set_partition_property(
                HvPartitionPropertyCode::IsolationState.0,
                mshv_bindings::hv_partition_isolation_state_HV_PARTITION_ISOLATION_SECURE as u64,
            )
            .map_err(|e| ErrorInner::SetPartitionProperty(e.into()))?;

        {
            let mut memory = self.memory.lock();
            for range in memory.ranges.iter_mut().flatten() {
                if !range.mapped {
                    self.vmfd
                        .map_user_memory(range.region)
                        .map_err(|e| SnpError::MapGuestMemory(e.into()))?;
                    range.mapped = true;
                }
            }
        }

        let mut batch_type = None;
        let mut batch = Vec::with_capacity(SNP_IMPORT_CHUNK_PAGES);
        for page in ordered_snp_import_pages(pages, snp.config.is_some()) {
            let Some(page_type) = snp_isolated_page_type(page.import_type)? else {
                continue;
            };
            validate_snp_page_range(page.range)?;
            for pfn in
                page.range.start() / hvdef::HV_PAGE_SIZE..page.range.end() / hvdef::HV_PAGE_SIZE
            {
                if batch_type != Some(page_type) || batch.len() == SNP_IMPORT_CHUNK_PAGES {
                    if let Some(current_type) = batch_type {
                        self.import_isolated_pages(current_type, &batch)?;
                        batch.clear();
                    }
                    batch_type = Some(page_type);
                }
                batch.push(pfn);
            }
        }
        if let Some(page_type) = batch_type {
            self.import_isolated_pages(page_type, &batch)?;
        }

        self.complete_isolated_import(snp.config.as_deref())?;

        let sev_control =
            mshv_bindings::snp::get_sev_control_register(vmsa_gpa / hvdef::HV_PAGE_SIZE);
        self.bsp_vcpufd
            .set_hvdef_regs(&[HvRegisterAssoc::from((
                HvX64RegisterName::SevControl,
                sev_control,
            ))])
            .map_err(ErrorInner::Register)?;
        Ok(())
    }

    fn write_snp_cpuid_page(&self, cpuid_gpa: u64) -> Result<(), Error> {
        let mut page = self
            .gm
            .read_plain::<x86defs::snp::HvPspCpuidPage>(cpuid_gpa)
            .map_err(SnpError::GuestMemory)?;
        let count = page.count as usize;
        if count > x86defs::snp::HV_PSP_CPUID_LEAF_COUNT_MAX {
            return Err(SnpError::TooManyCpuidEntries(count).into());
        }
        if self.caps.hv1 {
            // TODO: Determine the correct long-term strategy for exposing
            // synthetic Hyper-V CPUID leaves to direct-boot SNP guests: include
            // them in the measured CPUID page, rely on GHCB CPUID fallback, or
            // support both based on the guest contract.
            //
            // The loader creates this page with the architectural x86 and AMD
            // leaves needed by an SNP guest. Add the synthetic Hyper-V leaf
            // requests here because their values depend on the MSHV partition
            // configuration. The loop below queries MSHV for every requested
            // leaf and writes the results into this page before it is imported
            // through the SNP launch API. The PSP consequently measures these
            // synthetic values along with the rest of the CPUID page.
            //
            // This is not inherently required by the SNP or GHCB protocols. A
            // guest can treat a synthetic leaf absent from the measured table
            // as unsupported by that table and retry it through the GHCB CPUID
            // protocol. With CPUID VMGEXIT offloading enabled, the hypervisor
            // can answer that request using the CPUID intercept results
            // registered above. With offloading disabled, OpenVMM answers the
            // forwarded MSR or page-protocol request.
            //
            // Some Linux versions instead treat an absent, out-of-range
            // synthetic leaf as a successful all-zero result. In particular,
            // returning zeros for 0x40000000 prevents Hyper-V vendor detection
            // and therefore disables the entire Hyper-V guest interface. Keep
            // appending the leaves for compatibility with those guests. A
            // guest that returns -EOPNOTSUPP for missing synthetic leaves does
            // not require this augmentation.
            add_snp_hyperv_cpuid_leaves(&mut page).map_err(SnpError::TooManyCpuidEntries)?;
        }
        let count = page.count as usize;

        for leaf in &mut page.cpuid_leaf_info[..count] {
            let values = get_snp_cpuid_values(
                &self.bsp_vcpufd,
                leaf.eax_in,
                leaf.ecx_in,
                leaf.xfem_in,
                leaf.xss_in,
                self.caps.hv1,
            )
            .map_err(|e| SnpError::Cpuid(e.into()))?;
            leaf.eax_out = values[0];
            leaf.ebx_out = values[1];
            leaf.ecx_out = values[2];
            leaf.edx_out = values[3];
            leaf.reserved_z = 0;
        }
        self.gm
            .write_plain(cpuid_gpa, &page)
            .map_err(SnpError::GuestMemory)?;
        Ok(())
    }

    fn import_isolated_pages(&self, page_type: u8, pfns: &[u64]) -> Result<(), Error> {
        if pfns.is_empty() {
            return Ok(());
        }

        let mut buf =
            HeaderVec::<ImportIsolatedPagesHeader, u64, 0>::new(ImportIsolatedPagesHeader {
                page_type,
                rsvd: [0; 7],
                page_count: pfns.len() as u64,
            });
        buf.extend_tail_from_slice(pfns);
        // SAFETY: The custom header has the same C layout as
        // `mshv_import_isolated_pages`, followed by `page_count` u64 PFNs.
        let args = unsafe {
            &*buf
                .as_ptr()
                .cast::<mshv_bindings::mshv_import_isolated_pages>()
        };
        self.vmfd
            .import_isolated_pages(args)
            .map_err(|e| SnpError::ImportIsolatedPages(e.into()))?;
        Ok(())
    }

    fn complete_isolated_import(&self, config: Option<&MshvSnpConfig>) -> Result<(), Error> {
        let mut data = mshv_bindings::mshv_complete_isolated_import::default();
        let (parameters, snp_policy) = snp_launch_finish_data(config);
        tracing::info!(
            snp_policy,
            id_block_enabled = parameters.id_block_enabled != 0,
            vmsa_gpa = config.map(|config| config.vmsa_gpa),
            restricted_injection = config.map(|config| {
                x86defs::snp::SevFeatures::from(config.sev_features).restrict_injection()
            }),
            "completing MSHV SNP launch"
        );
        data.import_data.psp_parameters = parameters;
        self.vmfd
            .complete_isolated_import(&data)
            .map_err(|e| SnpError::CompleteIsolatedImport(e.into()))?;
        Ok(())
    }
}
