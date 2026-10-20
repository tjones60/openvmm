// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::Error;
use crate::KernelError;
use hvdef::HvPartitionPropertyCode;
use mshv_ioctls::VmFd;

const HVCALL_SET_PARTITION_PROPERTY_EX: u16 = 0x010a;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CcaError {
    #[error("failed to set processor features")]
    SetProcessorFeatures(#[source] KernelError),
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
