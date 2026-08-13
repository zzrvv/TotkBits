# `activeTab` types and their file formats

There are **10 active `activeTab` types** in the current UI. `ERROR` is a backend response value and does not become an active tab; `LOADING` only appears in commented-out code.

## `SARC`

- SARC archives (`.pack`, `.sarc`, and other SARC-backed files, optionally `.zs`)
- BARS audio archives (`.bars`, optionally `.zs`)
- BPHCL physics collision files (`.bphcl`)
- BPHHB physics files (`.bphhb`)
- HKCL Havok collision files (`.hkcl`)
- ZIP, 7-Zip, and RAR archives, plus opened folders (generic archive support)

## `YAML`

- AINB (`.ainb`, optionally `.zs`)
- ASB (`.asb`, optionally `.zs`)
- BYML-family files (`.byml`, `.bgyml`, `.sbyml`, Banc paths, and `GameDataList*`, optionally `.zs`)
- ESETB (`.esetb.byml`, optionally `.zs`)
- BFEVFL event-flow files (`.bfevfl`, optionally `.zs`)
- XLink/BELNK (`.belnk`, optionally `.zs`)
- Tag.Product files
- MSBT/MSYT message files (`.msbt`, `.msyt`)
- AAMP parameter files (`.baamp`, `.bparam`, `.aamp`)
- Super Mario Odyssey save files (detected by their binary layout)
- AMTA metadata entries inside BARS archives (opened as a read-only YAML preview)
- BPHCL leaf/node previews (opened as read-only YAML)
- Plain YAML, JSON, and text files (`.yaml`, `.yml`, `.json`, `.txt`)

## `RSTB`

- Resource size tables (`.rsizetable`, optionally `.zs`)

## `3D`

- BFRES models (`.bfres`, `.bfres.zs`, `.bfres.mc`)
- G1M models (`.g1m`)
- FBX models (`.fbx`)

## `IMAGE`

- BNTX texture containers (`.bntx`, optionally `.zs`)
- DDS textures (`.dds`)
- PNG, JPEG, TGA, and BMP raster images (`.png`, `.jpg`, `.jpeg`, `.tga`, `.bmp`)

## `AUDIO`

- BFWAV/BWAV audio entries (`.bfwav`, `.bwav`) opened from an archive

## `AMTA`

- No format currently activates this tab. An `AmtaView` exists, but AMTA entries currently activate `YAML` instead.

## `COMPARER`

- No dedicated file format. This is a format-independent comparison view used for supported archive entries and YAML/text-backed formats.

## `PHYSICS_MERGE`

- BPHCL
- BPHHB
- HKCL
- This is a merge utility rather than the primary opening tab for those formats; they normally open under `SARC`.

## `AOC_MODELS`

- No directly opened file format. This is a catalog view for models found in an AOC source; individual G1M models use `3D`.
