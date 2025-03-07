use super::liblz4::*;
use super::size_t;
use std::cmp;
use std::io::Result;
use std::io::Write;
use std::ptr;

#[derive(Debug)]
struct EncoderContext {
    c: LZ4FCompressionContext,
}

#[derive(Clone, Debug)]
pub struct EncoderBuilder {
    block_size: BlockSize,
    block_mode: BlockMode,
    checksum: ContentChecksum,
    // 0 == default (fast mode); values above 16 count as 16; values below 0 count as 0
    level: u32,
    // 1 == always flush (reduce need for tmp buffer)
    auto_flush: bool,
}

#[derive(Debug)]
pub struct Encoder<W> {
    c: EncoderContext,
    w: W,
    limit: usize,
    buffer: Vec<u8>,
}

impl EncoderBuilder {
    pub fn new() -> Self {
        EncoderBuilder {
            block_size: BlockSize::Default,
            block_mode: BlockMode::Linked,
            checksum: ContentChecksum::ChecksumEnabled,
            level: 0,
            auto_flush: false,
        }
    }

    pub fn block_size(&mut self, block_size: BlockSize) -> &mut Self {
        self.block_size = block_size;
        self
    }

    pub fn block_mode(&mut self, block_mode: BlockMode) -> &mut Self {
        self.block_mode = block_mode;
        self
    }

    pub fn checksum(&mut self, checksum: ContentChecksum) -> &mut Self {
        self.checksum = checksum;
        self
    }

    pub fn level(&mut self, level: u32) -> &mut Self {
        self.level = level;
        self
    }

    pub fn auto_flush(&mut self, auto_flush: bool) -> &mut Self {
        self.auto_flush = auto_flush;
        self
    }

    pub fn build<W: Write>(&self, w: W) -> Result<Encoder<W>> {
        let block_size = self.block_size.get_size();
        let preferences = LZ4FPreferences {
            frame_info: LZ4FFrameInfo {
                block_size_id: self.block_size.clone(),
                block_mode: self.block_mode.clone(),
                content_checksum_flag: self.checksum.clone(),
                reserved: [0; 5],
            },
            compression_level: self.level,
            auto_flush: if self.auto_flush { 1 } else { 0 },
            reserved: [0; 4],
        };
        let mut encoder = Encoder {
            w,
            c: EncoderContext::new()?,
            limit: block_size,
            buffer: Vec::with_capacity(check_error(unsafe {
                LZ4F_compressBound(block_size as size_t, &preferences)
            })?),
        };
        encoder.write_header(&preferences)?;
        Ok(encoder)
    }
}

impl<W: Write> Encoder<W> {
    fn write_header(&mut self, preferences: &LZ4FPreferences) -> Result<()> {
        unsafe {
            let len = check_error(LZ4F_compressBegin(
                self.c.c,
                self.buffer.as_mut_ptr(),
                self.buffer.capacity() as size_t,
                preferences,
            ))?;
            self.buffer.set_len(len);
        }
        self.w.write_all(&self.buffer)
    }

    fn write_end(&mut self) -> Result<()> {
        unsafe {
            let len = check_error(LZ4F_compressEnd(
                self.c.c,
                self.buffer.as_mut_ptr(),
                self.buffer.capacity() as size_t,
                ptr::null(),
            ))?;
            self.buffer.set_len(len);
        };
        self.w.write_all(&self.buffer)
    }

    /// Immutable writer reference.
    pub fn writer(&self) -> &W {
        &self.w
    }

    /// This function is used to flag that this session of compression is done
    /// with. The stream is finished up (final bytes are written), and then the
    /// wrapped writer is returned.
    pub fn finish(mut self) -> (W, Result<()>) {
        let result = self.write_end();
        (self.w, result)
    }
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        let mut offset = 0;
        while offset < buffer.len() {
            let size = cmp::min(buffer.len() - offset, self.limit);
            unsafe {
                let len = check_error(LZ4F_compressUpdate(
                    self.c.c,
                    self.buffer.as_mut_ptr(),
                    self.buffer.capacity() as size_t,
                    buffer[offset..].as_ptr(),
                    size as size_t,
                    ptr::null(),
                ))?;
                self.buffer.set_len(len);
                self.w.write_all(&self.buffer)?;
            }
            offset += size;
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> Result<()> {
        loop {
            unsafe {
                let len = check_error(LZ4F_flush(
                    self.c.c,
                    self.buffer.as_mut_ptr(),
                    self.buffer.capacity() as size_t,
                    ptr::null(),
                ))?;
                if len == 0 {
                    break;
                }
                self.buffer.set_len(len);
            };
            self.w.write_all(&self.buffer)?;
        }
        self.w.flush()
    }
}

impl EncoderContext {
    fn new() -> Result<EncoderContext> {
        let mut context = LZ4FCompressionContext(ptr::null_mut());
        check_error(unsafe { LZ4F_createCompressionContext(&mut context, LZ4F_VERSION) })?;
        Ok(EncoderContext { c: context })
    }
}

impl Drop for EncoderContext {
    fn drop(&mut self) {
        unsafe { LZ4F_freeCompressionContext(self.c) };
    }
}

#[cfg(test)]
mod test {
    use super::EncoderBuilder;
    use std::io::Write;

    #[test]
    fn test_encoder_smoke() {
        let mut encoder = EncoderBuilder::new().level(1).build(Vec::new()).unwrap();
        encoder.write(b"Some ").unwrap();
        encoder.write(b"data").unwrap();
        let (_, result) = encoder.finish();
        result.unwrap();
    }

    #[test]
    fn test_encoder_random() {
        let mut encoder = EncoderBuilder::new().level(1).build(Vec::new()).unwrap();
        let mut buffer = Vec::new();
        let mut rnd: u32 = 42;
        for _ in 0..1024 * 1024 {
            buffer.push((rnd & 0xFF) as u8);
            rnd = ((1664525 as u64) * (rnd as u64) + (1013904223 as u64)) as u32;
        }
        encoder.write(&buffer).unwrap();
        let (_, result) = encoder.finish();
        result.unwrap();
    }

    #[test]
    fn test_encoder_send() {
        fn check_send<S: Send>(_: &S) {}
        let enc = EncoderBuilder::new().build(Vec::new());
        check_send(&enc);
    }
}
