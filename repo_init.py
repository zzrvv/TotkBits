import subprocess
from pathlib import Path
import shutil
import os, sys, stat
from time import time
import requests
from tauri_build import build_dotnet

try:
    from tqdm import tqdm  # type: ignore
except ImportError:
    print("Install tqdm first using command: pip install tqdm")
    sys.exit(1)
CWD = Path(__file__).parent.resolve()


def download_files():
    files = {
        "https://github.com/SolidLink95/xlink2_bindings_rs/releases/download/0.1/xlink_tool.dll": "src-tauri/bin/dlls/xlink_tool.dll",
        "https://github.com/SolidLink95/xlink2_bindings_rs/releases/download/0.1/xlink_tool.exp": "src-tauri/bin/dlls/xlink_tool.exp",
        "https://github.com/SolidLink95/xlink2_bindings_rs/releases/download/0.1/xlink_tool.lib": "src-tauri/bin/dlls/xlink_tool.lib",
        "https://github.com/SolidLink95/oead/releases/download/v1.0/oead_byml_pipe.exe": "src-tauri/bin/cpp/oead_byml_pipe.exe",
    }
    if not files:
        return
    print("[+] Downloading files")
    for url, local_path in files.items():
        local_path = Path(local_path)
        local_path.parent.mkdir(parents=True, exist_ok=True)
        download_file(url, local_path)
    print(f"[+] Downloaded {len(files.keys())} files")


def remove_file(file):
    x = Path(file)
    if x.is_file():
        file_str = str(x)
        try:
            subprocess.run(["cmd", "/c", "del", file_str], check=True)
            print(f"[+] Removed: {file_str}")
        except subprocess.CalledProcessError:
            print(f"[-] Failed to remove: {file_str}")


def rename_directory(source, new_name):
    source_path = Path(source)
    new_path = source_path.parent / new_name
    if not new_path.exists():
        source_path.rename(new_path)
        print(f"Directory renamed from {source_path} to {new_path}")
    return new_path


def download_file(url, local_path):
    # Create the directory if it doesn't exist
    Path(local_path).parent.mkdir(parents=True, exist_ok=True)

    # Send a GET request to the URL
    response = requests.get(url, stream=True)
    # Check if the request was successful
    response.raise_for_status()

    # Get the total file size from the response headers
    total_size = int(response.headers.get("content-length", 0))
    local_path = str(local_path)
    # Open a local file for writing in binary mode
    with open(local_path, "wb") as f, tqdm(
        desc=local_path,
        total=total_size,
        unit="iB",
        unit_scale=True,
        unit_divisor=1024,
    ) as bar:
        # Write the response content to the local file in chunks
        for chunk in response.iter_content(chunk_size=8192):
            f.write(chunk)
            bar.update(len(chunk))
    return local_path


def copy_files(bin_path):
    files_to_copy = {}
    for file1, file2 in files_to_copy.items():
        shutil.copyfile(file1, file2)
        print(f"[+] Copied {file1} -> {file2}")


def repo_init():
    cwd_path = Path(__file__).parent
    cwd = str(cwd_path)
    bin_path = "src-tauri/bin"
    bin_path_p = Path(bin_path)
    if bin_path_p.exists() and bin_path_p.is_dir():
        shutil.rmtree(bin_path)
    bin_path_p.mkdir(parents=True, exist_ok=True)
    (cwd_path / "tmp").mkdir(parents=True, exist_ok=True)

    print(f"[+] Copying compressed json files")

    # Copy zlib compressed json files
    for file in (cwd_path / "src-tauri/misc").glob("*.bin"):
        if not file.is_file():
            continue
        destfile = cwd_path / "src-tauri/bin" / file.name
        if not destfile.is_file():
            print(f"Copying: {file.name}")
            shutil.copyfile(file, destfile)

    # Copy directories
    dirs_to_copy = {}
    for src_dir, dest_dir in dirs_to_copy.items():
        src_dir_path = Path(src_dir)
        dest_dir_path = Path(dest_dir)
        if not dest_dir_path.is_dir():
            shutil.copytree(src_dir_path, dest_dir_path)
            print(f"[+] Copied {src_dir} -> {dest_dir}")
        else:
            print(f"[+] Directory already exists: {dest_dir}")

    # Download dlls
    download_files()

    print(
        "\n[+] Totkbits initialized successfully. In order to build the project remember to install all other dependencies listed in README file"
    )


if __name__ == "__main__":
    repo_init()
