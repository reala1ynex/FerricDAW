# Configure Windows recording for Nintendo DS → External Microphone (Realtek).
# Prefers NirSoft SoundVolumeView (WinGet) for reliable default + subunit levels.

$ErrorActionPreference = 'Continue'

function Find-SoundVolumeView {
    $winget = Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Packages'
    if (Test-Path $winget) {
        $hit = Get-ChildItem $winget -Recurse -Filter SoundVolumeView.exe -ErrorAction SilentlyContinue |
            Select-Object -First 1 -ExpandProperty FullName
        if ($hit) { return $hit }
    }
    $cmd = Get-Command SoundVolumeView.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    return $null
}

$svv = Find-SoundVolumeView
if ($svv) {
    Write-Output "Using SoundVolumeView: $svv"
    $extId = 'Realtek(R) Audio\Device\External Microphone\Capture'
    & $svv /SetDefault $extId all
    & $svv /SetVolume $extId 100
    & $svv /Unmute $extId
    & $svv /SetVolume 'Realtek(R) Audio\Subunit\Microphone' 100
    & $svv /Unmute 'Realtek(R) Audio\Subunit\Microphone'
    # Boost at 100% turns the mic jack into a noise floor generator — leave at 0.
    & $svv /SetVolume 'Realtek(R) Audio\Subunit\Microphone Boost' 0
    Write-Output 'OK: External Microphone (Realtek) = default @ 100%, Mic Volume 100%, Mic Boost 0% (anti-hiss)'
    Write-Output 'HINT: Raise DS headphone volume; Ferric +6 dB trim max — never Mic Boost for Line-In'
    exit 0
}

Write-Output 'SoundVolumeView not found — falling back to COM PolicyConfig…'

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;

public static class FerricAudioFix {
    public enum EDataFlow { eRender = 0, eCapture = 1, eAll = 2 }
    public enum ERole { eConsole = 0, eMultimedia = 1, eCommunications = 2 }
    public enum DeviceState : uint { Active = 0x1, Disabled = 0x2, NotPresent = 0x4, Unplugged = 0x8 }

    [ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
    public class MMDeviceEnumerator { }

    [Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IMMDeviceEnumerator {
        int EnumAudioEndpoints(EDataFlow dataFlow, uint dwStateMask, out IMMDeviceCollection devices);
        int GetDefaultAudioEndpoint(EDataFlow dataFlow, ERole role, out IMMDevice endpoint);
        int GetDevice([MarshalAs(UnmanagedType.LPWStr)] string id, out IMMDevice device);
        int RegisterEndpointNotificationCallback(IntPtr client);
        int UnregisterEndpointNotificationCallback(IntPtr client);
    }

    [Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IMMDeviceCollection {
        int GetCount(out uint count);
        int Item(uint index, out IMMDevice device);
    }

    [Guid("D666063F-1587-4E43-81F1-B948E88031D4"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IMMDevice {
        int Activate(ref Guid iid, uint dwClsCtx, IntPtr pActivationParams, [MarshalAs(UnmanagedType.IUnknown)] out object ppInterface);
        int OpenPropertyStore(uint stgmAccess, out IPropertyStore props);
        int GetId([MarshalAs(UnmanagedType.LPWStr)] out string ppstrId);
        int GetState(out uint pdwState);
    }

    [Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IPropertyStore {
        int GetCount(out uint cProps);
        int GetAt(uint iProp, out PROPERTYKEY pkey);
        int GetValue(ref PROPERTYKEY key, out PROPVARIANT pv);
        int SetValue(ref PROPERTYKEY key, ref PROPVARIANT pv);
        int Commit();
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct PROPERTYKEY {
        public Guid fmtid;
        public uint pid;
    }

    [StructLayout(LayoutKind.Explicit)]
    public struct PROPVARIANT {
        [FieldOffset(0)] public ushort vt;
        [FieldOffset(8)] public IntPtr pointerValue;
    }

    [Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IAudioEndpointVolume {
        int RegisterControlChangeNotify(IntPtr p);
        int UnregisterControlChangeNotify(IntPtr p);
        int GetChannelCount(out uint pnChannelCount);
        int SetMasterVolumeLevel(float fLevelDB, ref Guid pguidEventContext);
        int SetMasterVolumeLevelScalar(float fLevel, ref Guid pguidEventContext);
        int GetMasterVolumeLevel(out float pfLevelDB);
        int GetMasterVolumeLevelScalar(out float pfLevel);
        int SetChannelVolumeLevel(uint nChannel, float fLevelDB, ref Guid pguidEventContext);
        int SetChannelVolumeLevelScalar(uint nChannel, float fLevel, ref Guid pguidEventContext);
        int GetChannelVolumeLevel(uint nChannel, out float pfLevelDB);
        int GetChannelVolumeLevelScalar(out float pfLevel);
        int SetMute([MarshalAs(UnmanagedType.Bool)] bool bMute, ref Guid pguidEventContext);
        int GetMute(out bool pbMute);
    }

    [ComImport, Guid("870AF99C-171D-4F9E-AF0D-E63DF40C2BC9")]
    public class PolicyConfigClient { }

    [Guid("F8679F50-850A-41CF-9C72-430F290290C8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IPolicyConfig {
        int GetMixFormat();
        int GetDeviceFormat();
        int ResetDeviceFormat();
        int SetDeviceFormat();
        int GetProcessingPeriod();
        int SetProcessingPeriod();
        int GetShareMode();
        int SetShareMode();
        int GetPropertyValue();
        int SetPropertyValue();
        int SetDefaultEndpoint([MarshalAs(UnmanagedType.LPWStr)] string deviceId, ERole role);
        int SetEndpointVisibility();
    }

    static string GetFriendlyName(IMMDevice dev) {
        IPropertyStore store;
        if (dev.OpenPropertyStore(0, out store) != 0) return "";
        var key = new PROPERTYKEY {
            fmtid = new Guid("a45c254e-df1c-4efd-8020-67d146a850e0"),
            pid = 2
        };
        PROPVARIANT pv;
        if (store.GetValue(ref key, out pv) != 0) return "";
        if (pv.vt == 31 && pv.pointerValue != IntPtr.Zero)
            return Marshal.PtrToStringUni(pv.pointerValue) ?? "";
        return "";
    }

    public static string ConfigureExternalMic() {
        var enumerator = (IMMDeviceEnumerator)(object)new MMDeviceEnumerator();
        IMMDeviceCollection coll;
        enumerator.EnumAudioEndpoints(EDataFlow.eCapture, (uint)DeviceState.Active, out coll);
        uint count;
        coll.GetCount(out count);

        IMMDevice chosen = null;
        string chosenName = null;
        string chosenId = null;
        var found = new List<string>();

        for (uint i = 0; i < count; i++) {
            IMMDevice d;
            coll.Item(i, out d);
            string name = GetFriendlyName(d);
            string id;
            d.GetId(out id);
            found.Add(name);
            string l = name.ToLowerInvariant();
            if (l.Contains("external microphone") && (l.Contains("realtek") || true)) {
                chosen = d; chosenName = name; chosenId = id;
                if (l.Contains("realtek")) break;
            }
        }

        if (chosen == null || chosenId == null)
            return "FAIL: No External Microphone found. Devices: " + string.Join(" | ", found);

        var policy = (IPolicyConfig)(object)new PolicyConfigClient();
        policy.SetDefaultEndpoint(chosenId, ERole.eConsole);
        policy.SetDefaultEndpoint(chosenId, ERole.eMultimedia);
        policy.SetDefaultEndpoint(chosenId, ERole.eCommunications);

        Guid iidVol = typeof(IAudioEndpointVolume).GUID;
        object oVol;
        chosen.Activate(ref iidVol, 1, IntPtr.Zero, out oVol);
        var vol = (IAudioEndpointVolume)oVol;
        Guid ctx = Guid.Empty;
        vol.SetMute(false, ref ctx);
        vol.SetMasterVolumeLevelScalar(1.0f, ref ctx);

        float scalar;
        vol.GetMasterVolumeLevelScalar(out scalar);
        return "OK: Default capture = " + chosenName + " @ " + (scalar * 100f).ToString("0") + "% unmuted";
    }
}
'@

$result = [FerricAudioFix]::ConfigureExternalMic()
Write-Output $result
Write-Output 'HINT: Sound → Recording → External Microphone → Levels → Mic + Boost max; Advanced → uncheck Exclusive Mode'
