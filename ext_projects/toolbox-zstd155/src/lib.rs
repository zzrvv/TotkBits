use std::slice;

#[no_mangle]
pub unsafe extern "C" fn toolbox_zstd155_bound(size: usize) -> usize {
    zstd::zstd_safe::compress_bound(size)
}

#[no_mangle]
pub unsafe extern "C" fn toolbox_zstd155_compress(
    source: *const u8,
    source_len: usize,
    target: *mut u8,
    target_len: usize,
) -> isize {
    if source.is_null() || target.is_null() {
        return -1;
    }
    let source = slice::from_raw_parts(source, source_len);
    let target = slice::from_raw_parts_mut(target, target_len);
    let result = (|| {
        let mut compressor = zstd::bulk::Compressor::new(20)?;
        compressor.set_parameter(zstd::zstd_safe::CParameter::ContentSizeFlag(false))?;
        compressor.set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(false))?;
        compressor.set_parameter(zstd::zstd_safe::CParameter::DictIdFlag(false))?;
        compressor.set_parameter(zstd::zstd_safe::CParameter::Format(
            zstd::zstd_safe::FrameFormat::Magicless,
        ))?;
        compressor.compress_to_buffer(source, target)
    })();
    result.map(|size| size as isize).unwrap_or(-1)
}
