$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

if ($env:OS -ne 'Windows_NT') {
    @{ sensor_os = 'not_windows' } | ConvertTo-Json -Compress | Write-Output
    exit 0
}

try {
    $collectNetworkDetails = __COLLECT_NETWORK_DETAILS__
    $collectGpuEngines = __COLLECT_GPU_ENGINES__
    $collectRamModules = __COLLECT_RAM_MODULES__
    $collectNetworkRates = __COLLECT_NETWORK_RATES__
    $collectCpuCores = __COLLECT_CPU_CORES__
    $collectDiskRates = __COLLECT_DISK_RATES__
    $collectThermals = __COLLECT_THERMALS__
    if ($collectNetworkDetails) {
        try {
            Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;
public static class ServerLauncherWlan {
    public static int ConnectionSize() { return Marshal.SizeOf(typeof(Connection)); }
    [StructLayout(LayoutKind.Sequential)] public struct Ssid { public uint length; [MarshalAs(UnmanagedType.ByValArray, SizeConst=32)] public byte[] bytes; }
    [StructLayout(LayoutKind.Sequential)] public struct Association { public Ssid ssid; public uint bss; [MarshalAs(UnmanagedType.ByValArray, SizeConst=6)] public byte[] bssid; public uint phy; public uint phyIndex; public uint signal; public uint rx; public uint tx; }
    [StructLayout(LayoutKind.Sequential)] public struct Security { public int enabled; public int oneX; public uint auth; public uint cipher; }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct Connection { public uint state; public uint mode; [MarshalAs(UnmanagedType.ByValArray, SizeConst=512)] public byte[] profile; public Association association; public Security security; }
    [DllImport("wlanapi.dll")] static extern uint WlanOpenHandle(uint version, IntPtr reserved, out uint negotiated, out IntPtr handle);
    [DllImport("wlanapi.dll")] static extern uint WlanQueryInterface(IntPtr handle, ref Guid id, uint opcode, IntPtr reserved, out uint size, out IntPtr data, IntPtr valueType);
    [DllImport("wlanapi.dll")] static extern void WlanFreeMemory(IntPtr memory);
    [DllImport("wlanapi.dll")] static extern uint WlanCloseHandle(IntPtr handle, IntPtr reserved);
    static string Auth(uint value) { switch(value) { case 1:return "Open"; case 2:return "Shared Key"; case 3:return "WPA-Enterprise"; case 4:return "WPA-Personal"; case 6:return "WPA2-Enterprise"; case 7:return "WPA2-Personal"; case 8:return "WPA3-Enterprise 192-bit"; case 9:return "WPA3-Personal"; case 10:return "Enhanced Open"; case 11:return "WPA3-Enterprise"; default:return "Other"; } }
    static string Cipher(uint value) { switch(value) { case 0:return "None"; case 1:return "WEP-40"; case 2:return "TKIP"; case 4:return "CCMP"; case 5:return "WEP-104"; case 6:return "BIP"; case 8:return "GCMP"; case 9:return "GCMP-256"; case 10:return "CCMP-256"; case 11:return "BIP-GMAC-128"; case 12:return "BIP-GMAC-256"; case 13:return "BIP-CMAC-256"; case 256:return "Group key"; case 257:return "WEP"; default:return "Other"; } }
    public static Dictionary<string, object> Read(string value) {
        uint negotiated; IntPtr handle;
        if (WlanOpenHandle(2, IntPtr.Zero, out negotiated, out handle) != 0) return null;
        try {
            Guid id = new Guid(value); uint size; IntPtr data;
            uint status = WlanQueryInterface(handle, ref id, 7, IntPtr.Zero, out size, out data, IntPtr.Zero);
            if (data == IntPtr.Zero) return null;
            try {
                if (status != 0 || size < Marshal.SizeOf(typeof(Connection))) return null;
                Connection c = (Connection)Marshal.PtrToStructure(data, typeof(Connection));
                Dictionary<string, object> result = new Dictionary<string, object>();
                result["state"] = c.state == 1 ? "CONNECTED" : c.state == 4 ? "DISCONNECTED" : "UNKNOWN";
                if (c.state != 1 || c.association.ssid.length > 32) return result;
                result["ssid"] = Encoding.UTF8.GetString(c.association.ssid.bytes, 0, (int)c.association.ssid.length);
                result["bssid"] = BitConverter.ToString(c.association.bssid).Replace('-', ':');
                if (c.association.signal <= 100) result["signal_pct"] = c.association.signal;
                result["rate"] = String.Format(CultureInfo.InvariantCulture, "RX {0:0.0} / TX {1:0.0} Mbps", c.association.rx / 1000.0, c.association.tx / 1000.0);
                result["security"] = Auth(c.security.auth) + " (" + Cipher(c.security.cipher) + ")";
                uint channelSize; IntPtr channelData;
                uint channelStatus = WlanQueryInterface(handle, ref id, 8, IntPtr.Zero, out channelSize, out channelData, IntPtr.Zero);
                if (channelData != IntPtr.Zero) {
                    try { if (channelStatus == 0 && channelSize >= 4) result["channel"] = (uint)Marshal.ReadInt32(channelData); }
                    finally { WlanFreeMemory(channelData); }
                }
                return result;
            } finally { WlanFreeMemory(data); }
        } finally { WlanCloseHandle(handle, IntPtr.Zero); }
    }
}
'@ -ErrorAction Stop | Out-Null
        } catch { }
    }
    $cpu = @(Get-CimInstance Win32_Processor -ErrorAction Stop)
    $cores = [int](($cpu | Measure-Object -Property NumberOfLogicalProcessors -Sum).Sum)
    if ($cores -lt 1) { throw 'No logical CPU count was reported' }
    $reportedPhysicalCores = @($cpu | Where-Object { $null -ne $_.NumberOfCores -and $_.NumberOfCores -gt 0 })
    $physicalCores = if ($reportedPhysicalCores.Count -gt 0) { [int](($reportedPhysicalCores | Measure-Object -Property NumberOfCores -Sum).Sum) } else { $null }
    $cpuLoad = $null
    $reportedCpuLoads = @($cpu | Where-Object { $null -ne $_.LoadPercentage })
    if ($reportedCpuLoads.Count -gt 0) { $cpuLoad = [double](($reportedCpuLoads | Measure-Object -Property LoadPercentage -Average).Average) }
    $cpuUsageCounters = @()
    try { $cpuUsageCounters = @(Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor -ErrorAction Stop) } catch { }
    if ($null -eq $cpuLoad) {
        $totalCpu = $cpuUsageCounters | Where-Object { $_.Name -eq '_Total' } | Select-Object -First 1
        if ($null -ne $totalCpu.PercentProcessorTime -and $totalCpu.PercentProcessorTime -ge 0 -and $totalCpu.PercentProcessorTime -le 100) {
            $cpuLoad = [double]$totalCpu.PercentProcessorTime
        }
    }
    $cpuCoreUsages = @()
    if ($collectCpuCores) {
        $coreRows = @($cpuUsageCounters |
            Where-Object { $_.Name -match '^\d+$' } |
            Sort-Object { [int]$_.Name } |
            ForEach-Object {
                $usage = $_.PercentProcessorTime
                [pscustomobject]@{
                    index = [int]$_.Name
                    usage = if ($null -ne $usage -and $usage -ge 0 -and $usage -le 100) { [double]$usage } else { $null }
                }
            })
        if ($coreRows.Count -gt 0) {
            $cpuCoreUsages = @($null) * ($coreRows[-1].index + 1)
            foreach ($row in $coreRows) { $cpuCoreUsages[$row.index] = $row.usage }
        }
    }
    $os = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
    $memoryTotal = [int64]$os.TotalVisibleMemorySize * 1024
    $memoryFree = [int64]$os.FreePhysicalMemory * 1024
    $memoryAvailable = $memoryFree
    try {
        $memoryCounter = Get-CimInstance Win32_PerfRawData_PerfOS_Memory -ErrorAction Stop
        if ($memoryCounter.AvailableBytes -gt 0) { $memoryAvailable = [int64]$memoryCounter.AvailableBytes }
    } catch { }
    $memoryModules = @()
    if ($collectRamModules) {
        try { $memoryModules = @(Get-CimInstance Win32_PhysicalMemory -ErrorAction Stop | Select-Object Manufacturer, Capacity, ConfiguredClockSpeed, SMBIOSMemoryType, FormFactor, DeviceLocator) } catch { }
    }

    $pageFiles = @(Get-CimInstance Win32_PageFileUsage -ErrorAction Stop)
    $swapTotal = [int64]((($pageFiles | Measure-Object -Property AllocatedBaseSize -Sum).Sum) * 1MB)
    $swapUsed = [int64]((($pageFiles | Measure-Object -Property CurrentUsage -Sum).Sum) * 1MB)
    $uptime = [int64]([DateTime]::Now - $os.LastBootUpTime).TotalSeconds
    $logicalDiskRates = @{}
    if ($collectDiskRates) {
        try {
            foreach ($sample in @(Get-CimInstance Win32_PerfFormattedData_PerfDisk_LogicalDisk -ErrorAction Stop)) {
                $read = $sample.DiskReadBytesPersec
                $write = $sample.DiskWriteBytesPersec
                if ($sample.Name -ne '_Total' -and $null -ne $read -and $read -ge 0 -and $null -ne $write -and $write -ge 0) {
                    $logicalDiskRates[[string]$sample.Name] = @([double]$read, [double]$write)
                }
            }
        } catch { $logicalDiskRates = @{} }
    }

    $disks = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' -ErrorAction Stop | ForEach-Object {
        if ($_.Size -gt 0 -and $_.FreeSpace -ge 0 -and $_.FreeSpace -le $_.Size) {
            $rates = $logicalDiskRates[[string]$_.DeviceID]
            [pscustomobject]@{
                name = [string]$_.DeviceID
                mount = [string]$_.DeviceID
                total = [int64]$_.Size
                used = [int64]($_.Size - $_.FreeSpace)
                avail = [int64]$_.FreeSpace
                read_rate = if ($null -ne $rates) { $rates[0] } else { $null }
                write_rate = if ($null -ne $rates) { $rates[1] } else { $null }
            }
        }
    })
    $totalDiskReadRate = $null
    $totalDiskWriteRate = $null
    if ($collectDiskRates) {
        try {
            $totalDisk = Get-CimInstance Win32_PerfFormattedData_PerfDisk_PhysicalDisk -ErrorAction Stop | Where-Object { $_.Name -eq '_Total' } | Select-Object -First 1
            if ($null -ne $totalDisk.DiskReadBytesPersec -and $totalDisk.DiskReadBytesPersec -ge 0) { $totalDiskReadRate = [double]$totalDisk.DiskReadBytesPersec }
            if ($null -ne $totalDisk.DiskWriteBytesPersec -and $totalDisk.DiskWriteBytesPersec -ge 0) { $totalDiskWriteRate = [double]$totalDisk.DiskWriteBytesPersec }
        } catch { }
    }
    $physicalDisks = @(Get-CimInstance Win32_DiskDrive -ErrorAction Stop | ForEach-Object {
        if ($_.Size -gt 0) {
            [pscustomobject]@{
                name = [string]$_.DeviceID
                model = [string]$_.Model
                bus_interface = [string]$_.InterfaceType
                smart_status = ''
                discard_supported = $null
                total_bytes = [int64]$_.Size
                is_ssd = $null
                partition_count = $null
                partitions = @()
                read_rate = $null
                write_rate = $null
            }
        }
    })

    $temps = @()
    $fans = @()
    if ($collectThermals) {
        $hardwareNames = @{}
        $hardwareTypes = @{}
        try {
            foreach ($device in @(Get-CimInstance -Namespace root/LibreHardwareMonitor -ClassName Hardware -ErrorAction Stop)) {
                if ($device.Identifier -and $device.Name) { $hardwareNames[[string]$device.Identifier] = [string]$device.Name }
                if ($device.Identifier -and $device.HardwareType) { $hardwareTypes[[string]$device.Identifier] = [string]$device.HardwareType }
            }
        } catch { }
        try {
            $hardwareSensors = @(Get-CimInstance -Namespace root/LibreHardwareMonitor -ClassName Sensor -ErrorAction Stop)
            $temps = @($hardwareSensors | Where-Object { $_.SensorType -eq 'Temperature' } | ForEach-Object {
                    $value = $_.Value
                    if ($null -ne $value -and $value -gt -50 -and $value -lt 200) {
                        $parent = [string]$_.Parent
                        [pscustomobject]@{
                            chip = if ($hardwareNames.ContainsKey($parent) -and $hardwareTypes.ContainsKey($parent)) { "$($hardwareNames[$parent]) ($($hardwareTypes[$parent]))" } elseif ($hardwareNames.ContainsKey($parent)) { $hardwareNames[$parent] } else { $parent }
                            label = [string]$_.Name
                            temp = [double]$value
                            crit = $null
                            max = $null
                        }
                    }
                })
            $fans = @($hardwareSensors | Where-Object { $_.SensorType -eq 'Fan' } | ForEach-Object {
                if ($null -ne $_.Value -and $_.Value -ge 0 -and $_.Value -le 100000) {
                    [pscustomobject]@{ chip=[string]$_.Parent; label=[string]$_.Name; rpm=[double]$_.Value }
                }
            })
        } catch { $temps = @(); $fans = @() }
    }

    $wlanText = $null
    if ($collectNetworkDetails) { try { $wlanText = (netsh wlan show interfaces 2>$null | Out-String) } catch { } }
    $adapters = @()
    $stats = @()
    try { $adapters = @(Get-NetAdapter -ErrorAction Stop) } catch { }
    $rateBefore = @{}
    $rateElapsed = $null
    if ($collectNetworkRates) {
        try {
            foreach ($sample in @(Get-NetAdapterStatistics -ErrorAction Stop)) {
                if ($null -ne $sample.ReceivedBytes -and $null -ne $sample.SentBytes) { $rateBefore[[string]$sample.Name] = @([int64]$sample.ReceivedBytes, [int64]$sample.SentBytes) }
            }
            $rateStartedAt = [Diagnostics.Stopwatch]::GetTimestamp()
            Start-Sleep -Milliseconds 250
            $rateElapsed = ([Diagnostics.Stopwatch]::GetTimestamp() - $rateStartedAt) / [Diagnostics.Stopwatch]::Frequency
        } catch { $rateBefore = @{} }
    }
    try { $stats = @(Get-NetAdapterStatistics -ErrorAction Stop) } catch { }
    $networks = @($adapters | ForEach-Object {
        $adapter = $_
        $stat = $stats | Where-Object { $_.Name -eq $adapter.Name } | Select-Object -First 1
        if ($null -ne $stat -and $null -ne $stat.ReceivedBytes -and $null -ne $stat.SentBytes -and $stat.ReceivedBytes -ge 0 -and $stat.SentBytes -ge 0) {
            $ipv4 = $null
            $ipv6 = $null
            $mtu = $null
            $wifiState = $null
            $wlan = $null
            if ($collectNetworkDetails -and $adapter.ifIndex) {
                $ipInterface = $null
                try { $ipInterface = Get-NetIPInterface -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4 -ErrorAction Stop | Select-Object -First 1 } catch { }
                if ($null -ne $ipInterface.NlMtu -and $ipInterface.NlMtu -gt 0) { $mtu = [uint32]$ipInterface.NlMtu }
                $ipv4Address = $null
                $ipv6Address = $null
                try { $ipv4Address = Get-NetIPAddress -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4 -ErrorAction Stop | Where-Object { $_.IPAddress -notlike '169.254.*' } | Select-Object -First 1 } catch { }
                try { $ipv6Address = Get-NetIPAddress -InterfaceIndex $adapter.ifIndex -AddressFamily IPv6 -ErrorAction Stop | Where-Object { $_.IPAddress -ne '::1' -and $_.IPAddress -notlike 'fe80:*' } | Select-Object -First 1 } catch { }
                if ($ipv4Address) { $ipv4 = "$($ipv4Address.IPAddress)/$($ipv4Address.PrefixLength)" }
                if ($ipv6Address) { $ipv6 = "$($ipv6Address.IPAddress)/$($ipv6Address.PrefixLength)" }
                if ([int]$adapter.ifType -eq 71) {
                    if ($adapter.InterfaceGuid) { try { $wlan = [ServerLauncherWlan]::Read([string]$adapter.InterfaceGuid) } catch { } }
                    $profile = $null
                    try { $profile = Get-NetConnectionProfile -InterfaceIndex $adapter.ifIndex -ErrorAction Stop | Select-Object -First 1 } catch { }
                    if ($null -ne $profile) { $wifiState = 'CONNECTED' }
                    elseif ($adapter.Status -eq 'Disconnected') { $wifiState = 'DISCONNECTED' }
                }
            }
            $mac = ([string]$adapter.MacAddress).Replace('-', ':').ToLowerInvariant()
            $linkState = if ($adapter.Status -eq 'Up') { 'up' } elseif ($adapter.Status -eq 'Disabled' -or $adapter.Status -eq 'Disconnected') { 'down' } else { 'unknown' }
            $rxRate = $null
            $txRate = $null
            if ($collectNetworkRates -and $null -ne $rateElapsed -and $rateElapsed -gt 0 -and $rateBefore.ContainsKey([string]$adapter.Name)) {
                $previous = $rateBefore[[string]$adapter.Name]
                if ($stat.ReceivedBytes -ge $previous[0] -and $stat.SentBytes -ge $previous[1]) {
                    $rxRate = [double]($stat.ReceivedBytes - $previous[0]) / $rateElapsed
                    $txRate = [double]($stat.SentBytes - $previous[1]) / $rateElapsed
                }
            }
            [pscustomobject]@{
                name = [string]$adapter.Name
                rx_bytes = [int64]$stat.ReceivedBytes
                tx_bytes = [int64]$stat.SentBytes
                rx_rate = $rxRate
                tx_rate = $txRate
                is_wireless = if ($null -ne $adapter.ifType) { [int]$adapter.ifType -eq 71 } else { $null }
                wifi_state = $wifiState
                wlan = $wlan
                link_state = $linkState
                mac_address = if ($mac -match '^([0-9a-f]{2}:){5}[0-9a-f]{2}$') { $mac } else { $null }
                mtu = $mtu
                ipv4 = $ipv4
                ipv6 = $ipv6
            }
        }
    })
    $networkStatsAvailable = $adapters.Count -gt 0 -and $networks.Count -eq $adapters.Count
    $networkRatesAvailable = $collectNetworkRates -and $networkStatsAvailable -and @($networks | Where-Object { $null -eq $_.rx_rate -or $null -eq $_.tx_rate }).Count -eq 0

    $batteries = @(Get-CimInstance Win32_Battery -ErrorAction Stop | ForEach-Object {
        $battery = $_
        $entry = [ordered]@{
            name = if ($battery.Name) { [string]$battery.Name } else { 'Battery' }
            status = if ($battery.BatteryStatus -eq 1) { 'Discharging' } elseif ($battery.BatteryStatus -eq 2) { 'AC connected' } elseif ($battery.BatteryStatus -eq 3) { 'Fully charged' } elseif ($battery.BatteryStatus -in 6, 7, 8, 9) { 'Charging' } elseif ($battery.BatteryStatus -eq 4) { 'Low' } elseif ($battery.BatteryStatus -eq 5) { 'Critical' } elseif ($battery.BatteryStatus -eq 11) { 'Partially charged' } else { 'Unknown' }
            capacity_pct = if ($null -ne $battery.EstimatedChargeRemaining -and $battery.EstimatedChargeRemaining -le 100) { [double]$battery.EstimatedChargeRemaining } else { $null }
            power_watts = $null
            voltage_volts = if ($battery.DesignVoltage -gt 0) { [double]$battery.DesignVoltage / 1000 } else { $null }
            energy_wh = $null
            energy_full_wh = if ($battery.FullChargeCapacity -gt 0) { [double]$battery.FullChargeCapacity / 1000 } else { $null }
            energy_design_wh = if ($battery.DesignCapacity -gt 0) { [double]$battery.DesignCapacity / 1000 } else { $null }
            energy_estimated = $false
            health_pct = $null
            cycle_count = $null
            technology = ''
            model_name = [string]$battery.Name
            manufacturer = [string]$battery.Manufacturer
        }
        if ($entry.capacity_pct -ne $null -and $entry.energy_full_wh -gt 0) {
            $entry.energy_wh = $entry.capacity_pct * $entry.energy_full_wh / 100
            $entry.energy_estimated = $true
        }
        if ($entry.energy_full_wh -gt 0 -and $entry.energy_design_wh -gt 0) { $entry.health_pct = [math]::Min(100, 100 * $entry.energy_full_wh / $entry.energy_design_wh) }
        [pscustomobject]$entry
    })

    $gpus = @(Get-CimInstance Win32_VideoController -ErrorAction Stop | ForEach-Object {
        if ($_.Name) {
            [pscustomobject]@{
                device_path = [string]$_.PNPDeviceID
                name = [string]$_.Name
                driver = [string]$_.DriverVersion
                load = $null
                temp = $null
                mem_used = $null
                mem_total = $null
                clock_mhz = $null
                mem_clock_mhz = $null
                power_watts = $null
                power_limit_watts = $null
            }
        }
    })
    $gpuEngines = @()
    if ($collectGpuEngines) {
        try { $gpuEngines = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -ErrorAction Stop | Select-Object Name, UtilizationPercentage) } catch { }
    }
    $processCount = @(Get-Process -ErrorAction SilentlyContinue).Count
    $processesCollected = $false
    $processNetworkCollected = $false
    $topApps = @()
    $collectProcesses = __COLLECT_PROCESSES__
    if ($collectProcesses) {
        try {
            function Read-ProcessSample {
                @(Get-Process -ErrorAction Stop | ForEach-Object {
                    $startMarker = $null
                    $cpuSeconds = $null
                    try { $startMarker = [int64]$_.StartTime.ToUniversalTime().ToFileTimeUtc() } catch { }
                    try { $cpuSeconds = [double]$_.CPU } catch { }
                    [pscustomobject]@{
                        pid = [uint32]$_.Id
                        name = [string]$_.ProcessName
                        start_marker = $startMarker
                        cpu_seconds = $cpuSeconds
                        mem_bytes = [int64]$_.WorkingSet64
                    }
                })
            }
            $sampleStartedAt = [Diagnostics.Stopwatch]::GetTimestamp()
            $before = @(Read-ProcessSample)
            Start-Sleep -Milliseconds 200
            $after = @(Read-ProcessSample)
            $elapsed = ([Diagnostics.Stopwatch]::GetTimestamp() - $sampleStartedAt) / [Diagnostics.Stopwatch]::Frequency
            $beforeByIdentity = @{}
            foreach ($process in $before) {
                if ($null -ne $process.start_marker) { $beforeByIdentity["$($process.pid):$($process.start_marker)"] = $process }
            }
            $topApps = @($after | ForEach-Object {
                $process = $_
                $prior = $null
                if ($null -ne $process.start_marker) { $prior = $beforeByIdentity["$($process.pid):$($process.start_marker)"] }
                $cpu = $null
                if ($prior -and $null -ne $prior.cpu_seconds -and $null -ne $process.cpu_seconds -and $elapsed -gt 0 -and $process.cpu_seconds -ge $prior.cpu_seconds) {
                    $cpu = [math]::Min(100, [math]::Max(0, 100 * ($process.cpu_seconds - $prior.cpu_seconds) / ($elapsed * [math]::Max(1, $cores))))
                }
                [pscustomobject]@{
                    pid = $process.pid
                    name = $process.name
                    start_marker = $process.start_marker
                    cpu = $cpu
                    mem_bytes = $process.mem_bytes
                    status = 'Unknown'
                    net_rx_bytes = $null
                    net_tx_bytes = $null
                    net_rate = $null
                    net_rate_available = $false
                    disk_read_bytes = $null
                    disk_write_bytes = $null
                    disk_read_rate = $null
                    disk_write_rate = $null
                    disk_rate = $null
                    disk_rate_available = $false
                }
            })
            $processCount = $after.Count
            $processesCollected = $true
        } catch { }
    }
    $tcpCount = $null
    $udpCount = $null
    try { $tcpCount = @(Get-NetTCPConnection -ErrorAction Stop).Count } catch { }
    try { $udpCount = @(Get-NetUDPEndpoint -ErrorAction Stop).Count } catch { }

    $result = [ordered]@{
        cpu_model = (($cpu | ForEach-Object { $_.Name } | Where-Object { $_ } | Select-Object -Unique) -join ' / ')
        cores = $cores
        physical_cores = $physicalCores
        cpu_usage = $cpuLoad
        cpu_core_usages = $cpuCoreUsages
        cpu_freqs = @($cpu | Where-Object { $_.CurrentClockSpeed -gt 0 } | ForEach-Object { [int64]$_.CurrentClockSpeed })
        cpu_power_cap = $null
        cpu_power = $null
        mem_total = $memoryTotal
        mem_avail = [int64][math]::Min($memoryAvailable, $memoryTotal)
        mem_free = $memoryFree
        swap_total = $swapTotal
        swap_free = [int64][math]::Max([int64]0, $swapTotal - $swapUsed)
        disks = $disks
        total_disk_read_rate = $totalDiskReadRate
        total_disk_write_rate = $totalDiskWriteRate
        temps = $temps
        fans = $fans
        gpus = $gpus
        gpu_engines = $gpuEngines
        memory_modules = $memoryModules
        networks = $networks
        network_stats_available = $networkStatsAvailable
        network_rates_available = $networkRatesAvailable
        wlan_text = $wlanText
        total_rx_rate = if ($networkRatesAvailable) { [double](($networks | Measure-Object -Property rx_rate -Sum).Sum) } else { $null }
        total_tx_rate = if ($networkRatesAvailable) { [double](($networks | Measure-Object -Property tx_rate -Sum).Sum) } else { $null }
        active_sockets = $null
        power = @{ ac_online = $null; batteries = $batteries; charge_limit = $null }
        top_apps = $topApps
        processes_collected = $processesCollected
        process_network_collected = $processNetworkCollected
        process_network_supported = $false
        process_count = $processCount
        os_name = [string]$os.Caption
        os_version = [string]$os.Version
        uptime = [int64][math]::Max([int64]0, $uptime)
        socket_stats = if ($null -ne $tcpCount -and $null -ne $udpCount) { @($tcpCount + $udpCount, $tcpCount, $udpCount) } else { $null }
        cpu_governor = ''
        physical_disks = $physicalDisks
        network_details_collected = [bool]$collectNetworkDetails
    }
    $result | ConvertTo-Json -Compress -Depth 8 | Write-Output
} catch {
    @{ error = "Windows sensor query failed: $($_.Exception.Message)" } | ConvertTo-Json -Compress | Write-Output
}
