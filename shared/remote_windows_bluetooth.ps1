$ErrorActionPreference='Stop'
$source = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class LauncherBluetooth {
    [StructLayout(LayoutKind.Sequential)]
    struct SearchParams {
        public uint Size;
        [MarshalAs(UnmanagedType.Bool)] public bool Authenticated;
        [MarshalAs(UnmanagedType.Bool)] public bool Remembered;
        [MarshalAs(UnmanagedType.Bool)] public bool Unknown;
        [MarshalAs(UnmanagedType.Bool)] public bool Connected;
        [MarshalAs(UnmanagedType.Bool)] public bool Inquiry;
        public byte Timeout;
        public IntPtr Radio;
    }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct DeviceInfo {
        public uint Size;
        public ulong Address;
        public uint ClassOfDevice;
        [MarshalAs(UnmanagedType.Bool)] public bool IsConnected;
        [MarshalAs(UnmanagedType.Bool)] public bool IsRemembered;
        [MarshalAs(UnmanagedType.Bool)] public bool IsAuthenticated;
        public SystemTime LastSeen;
        public SystemTime LastUsed;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 248)] public string Name;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct SystemTime {
        public ushort Year, Month, DayOfWeek, Day, Hour, Minute, Second, Milliseconds;
    }
    public sealed class Device {
        public string mac { get; set; }
        public string name { get; set; }
        public bool paired { get; set; }
        public bool connected { get; set; }
    }
    [DllImport("bthprops.cpl", ExactSpelling=true, SetLastError=true)]
    static extern IntPtr BluetoothFindFirstDevice(ref SearchParams search, ref DeviceInfo info);
    [DllImport("bthprops.cpl", ExactSpelling=true, SetLastError=true)]
    static extern bool BluetoothFindNextDevice(IntPtr find, ref DeviceInfo info);
    [DllImport("bthprops.cpl", ExactSpelling=true, SetLastError=true)]
    static extern bool BluetoothFindDeviceClose(IntPtr find);
    public static Device[] GetRememberedDevices() {
        var results = new List<Device>();
        var search = new SearchParams {
            Size = (uint)Marshal.SizeOf(typeof(SearchParams)), Authenticated = true,
            Remembered = true, Connected = true, Inquiry = false, Radio = IntPtr.Zero
        };
        var info = new DeviceInfo { Size = (uint)Marshal.SizeOf(typeof(DeviceInfo)) };
        IntPtr find = BluetoothFindFirstDevice(ref search, ref info);
        if (find == IntPtr.Zero) return results.ToArray();
        try {
            do {
                if (info.IsAuthenticated) {
                    ulong address = info.Address & 0xFFFFFFFFFFFFUL;
                    string mac = address.ToString("X12");
                    mac = string.Join(":", new string[] { mac.Substring(0,2), mac.Substring(2,2), mac.Substring(4,2), mac.Substring(6,2), mac.Substring(8,2), mac.Substring(10,2) });
                    results.Add(new Device { mac = mac, name = String.IsNullOrWhiteSpace(info.Name) ? mac : info.Name.Trim(), paired = true, connected = info.IsConnected });
                }
                info.Size = (uint)Marshal.SizeOf(typeof(DeviceInfo));
            } while (BluetoothFindNextDevice(find, ref info));
        } finally { BluetoothFindDeviceClose(find); }
        return results.ToArray();
    }
}
'@
Add-Type -TypeDefinition $source -Language CSharp
$devices = @([LauncherBluetooth]::GetRememberedDevices())
[pscustomobject]@{ available = $true; devices = $devices } | ConvertTo-Json -Compress -Depth 4
