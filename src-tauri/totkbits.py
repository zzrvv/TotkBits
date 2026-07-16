import io
import os
import sys
import tempfile
import traceback
import time
from typing import Dict
import oead
import evfl
CWD = os.getcwd()
sys.path.append(os.path.join(CWD, "bin/ainb"))
sys.path.append(os.path.join(CWD, "bin/asb"))
sys.path.append(os.path.join(CWD, "bin/ptcl"))

def log(message):
    """Diagnostics belong on stderr; stdout is the command's data protocol."""
    print(f"[totkbits.py] {message}", file=sys.stderr, flush=True)

log(f"startup cwd={CWD!r} python={sys.executable!r} argv={sys.argv!r}")

try:
    import ainb as ainb_lib
    # from bin.ainb.ainb.converter import ainb_to_json, json_to_ainb, ainb_to_yaml, yaml_to_ainb
except ImportError as e:
    ainb_lib = None
    log(f"AINB import failed: {e!r}")
    traceback.print_exc(file=sys.stderr)
try:
    from bin.asb.asb import ASB, asb_from_zs
except ImportError as e:
    ASB = None
    asb_from_zs = None
    log(f"ASB import failed: {e!r}")
    traceback.print_exc(file=sys.stderr)
try:
    from bin.ptcl.ptcl  import ptcl_binary_to_text_lib, ptcl_apply_edits_lib
except ImportError as e:
    ptcl_binary_to_text_lib = None
    ptcl_apply_edits_lib = None
    log(f"PTCL import failed: {e!r}")
    traceback.print_exc(file=sys.stderr)
import json
try:
    import yaml
except ImportError:
    raise ImportError("DID YOU EVEN TRY TO READ THE INSTRUCTIONS BEFORE YOU DID THIS? GO BACK TO THE GITHUB README AND LEARN TO READ :P")

    
# def evfl_text_to_binary(encoding="utf-8"): # Converts input JSON file to ASB
#     try:
#         data = sys.stdin.buffer.read()
#         json_data = yaml.safe_load(data.decode(encoding))
#         flow = evfl.EventFlow()
#         flow.read_json(json_data)
        
      
#         sys.stdout.buffer.write(new_rawdata.encode(encoding) if isinstance(new_rawdata, str) else new_rawdata)
     
#     except Exception as e:
#         sys.stdout.buffer.write(b"Error: " + str(e).encode(encoding))

    
def ptcl_text_to_binary(encoding="utf-8"): # Converts input JSON file to ASB
    try:
        merged_data = sys.stdin.buffer.read()
        if b"%PTCL_JSON%" not in merged_data:
            sys.stdout.buffer.write(b"Error: %PTCL_JSON% not found in input data")
            return
        ptcl_data, json_data = merged_data.split(b"%PTCL_JSON%")
        new_rawdata = ptcl_apply_edits_lib(ptcl_data, json_data)
        sys.stdout.buffer.write(new_rawdata.encode(encoding) if isinstance(new_rawdata, str) else new_rawdata)
     
    except Exception as e:
        sys.stdout.buffer.write(b"Error: " + str(e).encode(encoding))

def ptcl_binary_to_text() -> str: # Converts input PTCL file to JSON
    try:
        data = sys.stdin.buffer.read()
        text = ptcl_binary_to_text_lib(data)
        sys.stdout.buffer.write(text if isinstance(text, bytes) else text.encode("utf-8"))
    except Exception as e:
        sys.stdout.buffer.write(b"Error: " + str(e).encode("utf-8"))
        
def evfl_binary_to_text() -> str: # Converts input PTCL file to JSON
    #TODO: Implement this function
    try:
        data = sys.stdin.buffer.read()
        
        text = ptcl_binary_to_text_lib(data)
        sys.stdout.buffer.write(text if isinstance(text, bytes) else text.encode("utf-8"))
    except Exception as e:
        sys.stdout.buffer.write(b"Error: " + str(e).encode("utf-8"))


def asb_binary_to_text(): # Converts input ASB file to JSON
    try:
        data = sys.stdin.buffer.read()
        if not data.startswith(b"ASB"):
            sys.stdout.buffer.write(b"Error: invalid ASB magic")     
            return   
        asb = ASB(data, from_json=False)
        text = yaml.dump(asb.output_dict, sort_keys=False, allow_unicode=True, indent=4, encoding='utf-8')
        # text = yaml.dump(asb.output_dict, sort_keys=False, allow_unicode=True, indent=4, encoding='utf-8')
        sys.stdout.buffer.write(text.encode("utf-8") if isinstance(text, str) else text)
    except Exception as e:
        sys.stdout.buffer.write(b"Error binary_to_text: " + str(e).encode("utf-8"))

def asb_text_to_binary(encoding="utf-8"): # Converts input JSON file to ASB
    try:
        # sys.stdout.buffer.write(b"Executing command: asb_text_to_binary in function body\n")
        data = sys.stdin.buffer.read()
        # json_data = json.loads(data.decode(encoding))
        json_data = yaml.safe_load(data.decode(encoding))
        
        asb = ASB(json_data, from_json=True)
        
        cursor = io.BytesIO(bytearray())
        asb.ToBytes(cursor)
        sys.stdout.buffer.write(cursor.getvalue())
    except Exception as e:
        sys.stdout.buffer.write(b"Error: " + str(e).encode(encoding))
    
def ainb_binary_to_text(): # Converts input AINB file to JSON
    stage = "read stdin"
    started = time.monotonic()
    try:
        data = sys.stdin.buffer.read()
        log(f"ainb_binary_to_text: received {len(data)} bytes")
        if ainb_lib is None:
            raise RuntimeError("AINB library import failed; see startup diagnostics")
        stage = "validate input"
        if not data.startswith(b"AIB "):
            raise ValueError("invalid AINB magic (expected AIB + space)")
        stage = "parse binary"
        file = ainb_lib.AINB.from_binary(data)
        log(f"ainb_binary_to_text: parsed version={file.version:#x} filename={file.filename!r} nodes={len(file.nodes)}")
        stage = "serialize YAML"
        text = yaml.dump(file.as_dict(), sort_keys=False, allow_unicode=True, indent=4, encoding='utf-8')
        log(f"ainb_binary_to_text: produced {len(text)} bytes in {time.monotonic() - started:.3f}s")
        sys.stdout.buffer.write(text)
    except Exception as e:
        log(f"ainb_binary_to_text failed during {stage}: {type(e).__name__}: {e}")
        traceback.print_exc(file=sys.stderr)
        sys.stdout.buffer.write(b"Error: " + str(e).encode("utf-8"))

def ainb_text_to_binary(encoding="utf-8"): # Converts input JSON file to AINB
    stage = "read stdin"
    started = time.monotonic()
    try:
        data = sys.stdin.buffer.read()
        log(f"ainb_text_to_binary: received {len(data)} bytes encoding={encoding}")
        if ainb_lib is None:
            raise RuntimeError("AINB library import failed; see startup diagnostics")
        stage = "decode text"
        decoded = data.decode(encoding)
        stage = "parse YAML"
        json_data = yaml.safe_load(decoded)
        if not isinstance(json_data, dict):
            raise ValueError(f"AINB YAML root must be a mapping, got {type(json_data).__name__}")
        log(f"ainb_text_to_binary: YAML parsed with {len(json_data)} top-level keys")
        stage = "construct AINB"
        file = ainb_lib.AINB.from_dict(json_data)
        stage = "serialize binary"
        result = file.to_binary()
        if not result.startswith(b"AIB "):
            raise ValueError("AINB serializer returned data with invalid magic")
        log(f"ainb_text_to_binary: produced {len(result)} bytes in {time.monotonic() - started:.3f}s")
        sys.stdout.buffer.write(result)
    except Exception as e:
        log(f"ainb_text_to_binary failed during {stage}: {type(e).__name__}: {e}")
        traceback.print_exc(file=sys.stderr)
        sys.stdout.buffer.write(b"Error: " + str(e).encode(encoding))

def byml_text_to_binary(encoding="utf-8"): # Converts input JSON file to AINB
    try:
        data = sys.stdin.buffer.read()
        str_var = data.decode(encoding)
        pio = oead.byml.from_text(str_var)
        rawdata = oead.byml.to_binary(pio, big_endian=False)
        sys.stdout.buffer.write(bytes(rawdata))
    except Exception as e:
        sys.stdout.buffer.write(b"Error: " + str(e).encode(encoding))

if __name__ == "__main__":
    if len(sys.argv) > 1:
        commands = {
            "ainb_binary_to_text": ainb_binary_to_text,
            "ainb_text_to_binary": ainb_text_to_binary,
            "asb_binary_to_text": asb_binary_to_text,
            "asb_text_to_binary": asb_text_to_binary,  
            "byml_text_to_binary": byml_text_to_binary,
            "ptcl_binary_to_text": ptcl_binary_to_text,
            "ptcl_text_to_binary": ptcl_text_to_binary
        }

        # Execute the function based on the command line argument
        if sys.argv[1] in commands.keys():
            # sys.stdout.write(f"Executing command '{sys.argv[1]}'\n")
            command = sys.argv[1]
            log(f"dispatch command={command!r}")
            commands[command]()
            log(f"command={command!r} completed")
        else:
            print(f"Command '{sys.argv[1]}' not recognized.")
    else:
        sys.stdout.write("Hello from python")
