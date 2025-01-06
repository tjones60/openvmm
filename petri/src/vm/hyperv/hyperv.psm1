$ROOT_HYPER_V_NAMESPACE = "root\virtualization\v2"

function Set-InitialMachineConfiguration
{
    [CmdletBinding()]
    Param (
        [Parameter(Mandatory = $true)]
        [string] $VMName,
        [Parameter(Mandatory = $true)]
        [string] $ImcHive
    )

    $msvm_ComputerSystem = Get-CimInstance -namespace $ROOT_HYPER_V_NAMESPACE -query "select * from Msvm_ComputerSystem where ElementName = '$VMName'"

    if ($msvm_ComputerSystem.Length -gt 1) {
        throw "More than one VM with name '$VMName' exists!"
    }

    if (-not $msvm_ComputerSystem)
    {
        throw "Unable to find a virtual machine with name $VMName."
    }

    $imcHiveData = Get-Content -Encoding Byte $ImcHive
    $length = [System.BitConverter]::GetBytes([int32]$imcHiveData.Length + 4)
    if ([System.BitConverter]::IsLittleEndian)
    {
        [System.Array]::Reverse($length);
    }
    $imcData = $length + $imcHiveData

    $vmms = Get-CimInstance -Namespace $ROOT_HYPER_V_NAMESPACE -Class Msvm_VirtualSystemManagementService
    $vmms | Invoke-CimMethod -name "SetInitialMachineConfigurationData" -Arguments @{
        "TargetSystem" = $msvm_ComputerSystem;
        "ImcData" = [byte[]]$imcData
    }
}

function Set-OpenHCLFirmware
{
    [CmdletBinding()]
    Param (
        [Parameter(Mandatory = $true)]
        [string] $VMName,
        [Parameter(Mandatory = $true)]
        [string] $IgvmFile
    )

    $msvm_ComputerSystem = Get-CimInstance -namespace $ROOT_HYPER_V_NAMESPACE -query "select * from Msvm_ComputerSystem where ElementName = '$VMName'"
    $vssd = $msvm_ComputerSystem | Get-CimAssociatedInstance -ResultClass "Msvm_VirtualSystemSettingData" -Association "Msvm_SettingsDefineState"
    # Enable OpenHCL by feature
    $vssd.GuestFeatureSet = 0x00000201
    # Set the OpenHCL image file path 
    $vssd.FirmwareFile = $Path

    $vmms = Get-CimInstance -Namespace $ROOT_HYPER_V_NAMESPACE -Class Msvm_VirtualSystemManagementService
    $cimSerializer = [Microsoft.Management.Infrastructure.Serialization.CimSerializer]::Create()
    $serializedObj = $cimSerializer.Serialize($vssd, [Microsoft.Management.Infrastructure.Serialization.InstanceSerializationOptions]::None)
    $serializedString = [System.Text.Encoding]::Unicode.GetString($serializedObj)
    $vmms | Invoke-CimMethod -Name "ModifySystemSettings" -Arguments @{
        "SystemSettings" = $serializedString
    }
}