// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use super::*;
use crate::KernelError;
use headervec::HeaderVec;
use mshv_ioctls::VmFd;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

const HVCALL_SET_PARTITION_PROPERTY_EX: u16 = 0x010a;
const CCA_IMPORT_CHUNK_PAGES: usize = 256;
const CCA_MAILBOX_GPA: u64 = 0x10_0000;
const MSHV_SET_MEM_BIT_SHARED: u8 = 3;

#[derive(Debug, Copy, Clone, Eq, PartialEq, inspect::Inspect)]
pub(crate) enum CcaLaunchState {
    NotStarted,
    Started,
    Finished,
    Failed,
}

#[derive(Debug, inspect::Inspect)]
pub(crate) struct CcaPartitionState {
    pub(crate) launch_state: parking_lot::Mutex<CcaLaunchState>,
    #[inspect(skip)]
    pending_registers: parking_lot::Mutex<Vec<HvRegisterAssoc>>,
    #[inspect(skip)]
    mailbox: Arc<CcaMailboxPage>,
}

impl CcaPartitionState {
    pub(crate) fn new() -> Self {
        Self {
            launch_state: CcaLaunchState::NotStarted.into(),
            pending_registers: Default::default(),
            mailbox: Arc::new(CcaMailboxPage::new()),
        }
    }

    pub(crate) fn defer_registers(&self, registers: &[HvRegisterAssoc]) -> bool {
        if *self.launch_state.lock() != CcaLaunchState::NotStarted {
            return false;
        }
        self.pending_registers.lock().extend(
            registers
                .iter()
                .filter(|register| {
                    register.name == HvArm64RegisterName::X0.into()
                        || register.name == HvArm64RegisterName::XPc.into()
                })
                .copied(),
        );
        true
    }
}

#[derive(Debug, Error)]
pub(crate) enum CcaError {
    #[error("CCA launch is already in progress")]
    LaunchInProgress,
    #[error("CCA launch previously failed")]
    LaunchFailed,
    #[error("unsupported CCA initial page import type: {0:?}")]
    UnsupportedPageImportType(virt::InitialPageImportType),
    #[error("invalid CCA initial page range")]
    InvalidPageRange,
    #[error("failed to set CCA processor features")]
    SetProcessorFeatures(#[source] KernelError),
    #[error("failed to map CCA guest memory")]
    MapGuestMemory(#[source] KernelError),
    #[error("failed to map CCA mailbox")]
    MapMailbox(#[source] KernelError),
    #[error("failed to query CCA physical address width")]
    PhysicalAddressWidth(#[source] KernelError),
    #[error("failed to import CCA pages")]
    ImportIsolatedPages(#[source] KernelError),
    #[error("failed to complete CCA isolated import")]
    CompleteIsolatedImport(#[source] KernelError),
}

#[repr(C)]
struct SetPartitionPropertyExInput {
    partition_id: u64,
    property_code: u32,
    padding: u32,
    arg: u64,
    bank_count: u8,
    reserved0: u8,
    reserved1: [u16; 3],
    disabled_features: [u64; 3],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct ImportIsolatedPagesHeader {
    page_type: u8,
    reserved: [u8; 7],
    page_count: u64,
}

#[repr(C, align(4096))]
#[derive(Debug)]
struct CcaMailboxPage {
    value: AtomicU64,
    padding: [u8; 4088],
}

impl CcaMailboxPage {
    fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
            padding: [0; 4088],
        }
    }
}

pub(super) fn set_disabled_processor_features(vmfd: &VmFd) -> Result<(), Error> {
    let input = SetPartitionPropertyExInput {
        partition_id: 0,
        property_code: HvPartitionPropertyCode::DisabledProcessorFeaturesEx.0,
        padding: 0,
        arg: 0,
        bank_count: 3,
        reserved0: 0,
        reserved1: [0; 3],
        disabled_features: [0; 3],
    };
    let mut args = mshv_bindings::mshv_root_hvcall {
        code: HVCALL_SET_PARTITION_PROPERTY_EX,
        in_sz: size_of_val(&input) as u16,
        in_ptr: std::ptr::addr_of!(input) as u64,
        ..Default::default()
    };
    vmfd.hvcall(&mut args)
        .map_err(|e| CcaError::SetProcessorFeatures(e.into()))?;
    Ok(())
}

fn cca_isolated_page_type(import_type: virt::InitialPageImportType) -> Result<u8, Error> {
    match import_type {
        virt::InitialPageImportType::Normal => Ok(mshv_bindings::MSHV_ISOLATED_PAGE_NORMAL as u8),
        virt::InitialPageImportType::NormalUnmeasured => {
            Ok(mshv_bindings::MSHV_ISOLATED_PAGE_UNMEASURED as u8)
        }
        _ => Err(CcaError::UnsupportedPageImportType(import_type).into()),
    }
}

fn validate_cca_page_range(range: memory_range::MemoryRange) -> Result<(), Error> {
    if range.is_empty()
        || !range.start().is_multiple_of(hvdef::HV_PAGE_SIZE)
        || !range.end().is_multiple_of(hvdef::HV_PAGE_SIZE)
    {
        return Err(CcaError::InvalidPageRange.into());
    }
    Ok(())
}
