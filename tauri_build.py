import os
import shutil
from pathlib import Path
import subprocess
import time
os.system('cls')

def clear_directory_contents(directory: Path, expected_parent: Path):
    directory = directory.resolve()
    expected_parent = expected_parent.resolve()
    if directory.parent != expected_parent:
        raise RuntimeError(f"Refusing to clear unexpected directory: {directory}")
    directory.mkdir(parents=True, exist_ok=True)
    for child in directory.iterdir():
        if child.is_dir() and not child.is_symlink():
            shutil.rmtree(child)
        else:
            child.unlink()

def find_7z_exe():
    found = shutil.which("7z.exe") or shutil.which("7z")
    if found:
        return Path(found)
    candidates = [
        Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "7-Zip/7z.exe",
        Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "7-Zip/7z.exe",
        Path(os.environ.get("LOCALAPPDATA", "")) / "Programs/7-Zip/7z.exe",
    ]
    return next((candidate for candidate in candidates if candidate.is_file()), None)

def install_and_package(cwd: Path):
    bundle_dir = cwd / "src-tauri/target/release/bundle/nsis"
    installers = sorted(bundle_dir.glob("*.exe"), key=lambda path: path.stat().st_mtime, reverse=True)
    if not installers:
        raise FileNotFoundError(f"No NSIS installer found in {bundle_dir}")

    install_dir = (cwd / "res/Totkbits").resolve()
    clear_directory_contents(install_dir, cwd / "res")
    installer = installers[0].resolve()
    print(f"[+] Silently installing {installer.name} to {install_dir}")
    run([str(installer), "/S", f"/D={install_dir}"])
    if not any(install_dir.iterdir()):
        raise RuntimeError(f"Installer completed but {install_dir} is empty")

    archive_7z = install_dir.with_suffix(".7z")
    archive_zip = install_dir.with_suffix(".zip")
    for archive in (archive_7z, archive_zip):
        if archive.exists():
            archive.unlink()

    seven_zip = find_7z_exe()
    if seven_zip:
        print(f"[+] Compressing installed folder with {seven_zip}")
        run([str(seven_zip), "a", "-t7z", "-mx=9", str(archive_7z), install_dir.name])
        run([str(seven_zip), "a", "-tzip", "-mx=9", str(archive_zip), install_dir.name])
    else:
        print("[-] 7z.exe not found; creating ZIP with Windows tar via cmd")
        run(["cmd", "/c", "tar", "-a", "-c", "-f", str(archive_zip), install_dir.name])
    print(f"[+] Packaged installed application: {archive_zip}")
    if archive_7z.exists():
        print(f"[+] Packaged installed application: {archive_7z}")

def run(cmd):
    # subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
    subprocess.run(cmd,  check=True)

def prepare_main_rs(cwd):
    main_rs = cwd / "src-tauri/src/main.rs"
    assert(main_rs.exists())
    data = main_rs.read_text()
    data = data.replace("""// #![windows_subsystem = "windows"]""", """#![windows_subsystem = "windows"]""")
    main_rs.write_text(data)

def restore_main_rs(cwd):
    main_rs = cwd / "src-tauri/src/main.rs"
    assert(main_rs.exists())
    data = main_rs.read_text()
    data = data.replace("""#![windows_subsystem = "windows"]""", """// #![windows_subsystem = "windows"]""")
    main_rs.write_text(data)

def tauri_build():
    t1 = time.time()
    os.system("cls")
    cwd  = Path(__file__).parent
    cwd_str = str(cwd)
    os.chdir(str(cwd / "src-tauri"))
    print(f"[+] Cleaning tauri project")
    run(["cargo", "clean"])
    print(f"[+] Preparing main.rs")
    prepare_main_rs(cwd)
    os.chdir(cwd_str)
    print(f"[+] Building tauri project")
    # run(["cargo", "tauri", "build",  "-- --release"])
    # The release workflow installs the NSIS executable below; building WiX as
    # well can fail independently and prevents the requested portable archives
    # from being produced even when the application itself compiled correctly.
    run(["cargo", "tauri", "build", "--bundles", "nsis"])
    print(f"[+] Restoring main.rs")
    restore_main_rs(cwd)
    os.chdir(str(cwd / "res"))
    install_and_package(cwd)
    os.chdir(cwd_str)
    subprocess.run(["cmd", "/c", "start", "explorer", str(cwd / "src-tauri\\target\\release\\bundle\\nsis")])
    t2 = time.time()
    mins = int((t2-t1) // 60)
    secs = int((t2-t1) % 60)
    print(f"[+] Done in {mins} mins {secs} secs")
    
if __name__ == "__main__":
    tauri_build()
    
