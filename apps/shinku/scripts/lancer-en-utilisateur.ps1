# Lance un programme avec les droits ordinaires d'un utilisateur, attend sa
# fin et rend son code de sortie.
#
# Sur les machines de l'intégration continue, tout tourne avec les droits
# d'un administrateur. Shinku, lui, se lance sans ces droits sur le PC de
# quelqu'un : c'est ainsi qu'il faut l'essayer. Et WebView2 n'ouvre son port
# de débogage à la demande d'une variable d'environnement que dans un
# programme sans droits d'administrateur.
#
# Le jeton donné au programme est celui de ce processus, filtré comme le fait
# le contrôle de compte d'utilisateur : groupe Administrateurs en refus seul,
# privilèges retirés, niveau d'intégrité moyen.
#
# La première ligne écrite est « PID <numéro> », celui du programme lancé.

param(
    [Parameter(Mandatory = $true)][string]$Programme,
    # Essai : laisse au jeton le propriétaire et les droits par défaut du jeton d'origine.
    [switch]$JetonBrut
)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;

public static class StandardUser
{
    const uint TOKEN_ALL_ACCESS = 0xF01FF;
    const uint LUA_TOKEN = 0x4;
    const int TokenUser = 1;
    const int TokenOwner = 4;
    const int TokenDefaultDacl = 6;
    const int TokenIntegrityLevel = 25;
    const int GENERIC_ALL = 0x10000000;
    const uint SE_GROUP_INTEGRITY = 0x20;
    const uint INFINITE = 0xFFFFFFFF;
    const string MEDIUM_INTEGRITY = "S-1-16-8192";

    [StructLayout(LayoutKind.Sequential)]
    struct SID_AND_ATTRIBUTES
    {
        public IntPtr Sid;
        public uint Attributes;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct TOKEN_MANDATORY_LABEL
    {
        public SID_AND_ATTRIBUTES Label;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct STARTUPINFO
    {
        public int cb;
        public string lpReserved, lpDesktop, lpTitle;
        public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2;
        public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct PROCESS_INFORMATION
    {
        public IntPtr hProcess, hThread;
        public int dwProcessId, dwThreadId;
    }

    [DllImport("kernel32.dll")]
    static extern IntPtr GetCurrentProcess();

    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);

    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool CreateRestrictedToken(
        IntPtr token, uint flags,
        uint disableCount, IntPtr disable,
        uint deleteCount, IntPtr delete,
        uint restrictCount, IntPtr restrict,
        out IntPtr restricted);

    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool ConvertStringSidToSid(string text, out IntPtr sid);

    [DllImport("advapi32.dll")]
    static extern uint GetLengthSid(IntPtr sid);

    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool GetTokenInformation(IntPtr token, int kind, IntPtr buffer, uint size, out uint needed);

    [DllImport("advapi32.dll", SetLastError = true, EntryPoint = "SetTokenInformation")]
    static extern bool SetTokenLabel(IntPtr token, int kind, ref TOKEN_MANDATORY_LABEL value, uint size);

    // TOKEN_OWNER et TOKEN_DEFAULT_DACL tiennent chacun en un pointeur.
    [DllImport("advapi32.dll", SetLastError = true, EntryPoint = "SetTokenInformation")]
    static extern bool SetTokenPointer(IntPtr token, int kind, ref IntPtr value, uint size);

    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool CreateProcessAsUser(
        IntPtr token, string application, StringBuilder commandLine,
        IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles, uint flags,
        IntPtr environment, string directory,
        ref STARTUPINFO startup, out PROCESS_INFORMATION created);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool GetExitCodeProcess(IntPtr process, out uint code);

    static IntPtr started;

    // Donne au jeton le proprietaire et les droits par defaut d'un utilisateur
    // ordinaire. Ceux du jeton d'origine designent le groupe Administrateurs,
    // que le jeton filtre ne peut plus faire valoir : sans cela, le programme
    // n'aurait pas acces aux objets qu'il cree lui-meme.
    static void OwnWhatItCreates(IntPtr token)
    {
        uint needed;
        GetTokenInformation(token, TokenUser, IntPtr.Zero, 0, out needed);
        IntPtr buffer = Marshal.AllocHGlobal((int)needed);
        if (!GetTokenInformation(token, TokenUser, buffer, needed, out needed))
        {
            throw new Win32Exception();
        }
        IntPtr user = Marshal.ReadIntPtr(buffer);
        if (!SetTokenPointer(token, TokenOwner, ref user, (uint)IntPtr.Size))
        {
            throw new Win32Exception();
        }

        RawAcl acl = new RawAcl(GenericAcl.AclRevision, 2);
        acl.InsertAce(0, new CommonAce(AceFlags.None, AceQualifier.AccessAllowed, GENERIC_ALL,
            new SecurityIdentifier(user), false, null));
        acl.InsertAce(1, new CommonAce(AceFlags.None, AceQualifier.AccessAllowed, GENERIC_ALL,
            new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null), false, null));
        byte[] bytes = new byte[acl.BinaryLength];
        acl.GetBinaryForm(bytes, 0);
        IntPtr dacl = Marshal.AllocHGlobal(bytes.Length);
        Marshal.Copy(bytes, 0, dacl, bytes.Length);
        if (!SetTokenPointer(token, TokenDefaultDacl, ref dacl, (uint)IntPtr.Size))
        {
            throw new Win32Exception();
        }
    }

    // Lance le programme et rend son numero de processus.
    public static int Start(string program, bool rawToken)
    {
        IntPtr own;
        if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, out own))
        {
            throw new Win32Exception();
        }
        IntPtr standard;
        if (!CreateRestrictedToken(own, LUA_TOKEN, 0, IntPtr.Zero, 0, IntPtr.Zero, 0, IntPtr.Zero, out standard))
        {
            throw new Win32Exception();
        }
        IntPtr medium;
        if (!ConvertStringSidToSid(MEDIUM_INTEGRITY, out medium))
        {
            throw new Win32Exception();
        }
        TOKEN_MANDATORY_LABEL label = new TOKEN_MANDATORY_LABEL();
        label.Label.Sid = medium;
        label.Label.Attributes = SE_GROUP_INTEGRITY;
        uint size = (uint)Marshal.SizeOf(label) + GetLengthSid(medium);
        if (!SetTokenLabel(standard, TokenIntegrityLevel, ref label, size))
        {
            throw new Win32Exception();
        }
        if (!rawToken)
        {
            OwnWhatItCreates(standard);
        }

        STARTUPINFO startup = new STARTUPINFO();
        startup.cb = Marshal.SizeOf(startup);
        PROCESS_INFORMATION created;
        StringBuilder commandLine = new StringBuilder("\"" + program + "\"");
        // Sans bloc d'environnement : le programme recoit celui de ce processus.
        if (!CreateProcessAsUser(standard, program, commandLine, IntPtr.Zero, IntPtr.Zero, false, 0,
                IntPtr.Zero, null, ref startup, out created))
        {
            throw new Win32Exception();
        }
        started = created.hProcess;
        return created.dwProcessId;
    }

    // Attend la fin du programme et rend son code de sortie.
    public static int Wait()
    {
        WaitForSingleObject(started, INFINITE);
        uint code;
        if (!GetExitCodeProcess(started, out code))
        {
            throw new Win32Exception();
        }
        return unchecked((int)code);
    }
}
'@

try {
    $id = [StandardUser]::Start($Programme, $JetonBrut.IsPresent)
} catch {
    # Windows a refusé de créer le processus : sa raison est le message.
    [Console]::Error.WriteLine($_.Exception.GetBaseException().Message)
    exit 1
}
[Console]::Out.WriteLine("PID $id")
[Console]::Out.Flush()
exit ([StandardUser]::Wait())
