


Look at the repo W:\coding\TotkBits\tmp\MeshCodec . Add code changes: user needs to be able to build the dll file 
with 3 functions: meshcodec_decompress (take char* and int len bytes as args, decompress them,  return char* and int len), meshcodec_compress (take char* and int len bytes as args, compress them,  return char* and int len), meshcodec_free (free char* and any other relevant pointers). Create MANUAL.MD file - explain which parts of cmake are used to conduct building dll and explain additional code used to expose relevant functions. Explain in MANUAL.MD building prerequisities and dependencies and commands needed to build dll. Then build the dll and test it on meshcodec compressed files in W:\coding\TotkBits\tmp\mcpk