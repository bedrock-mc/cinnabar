//! Decoder process entry: one start record, signed chunks on request, decoded output back.

use super::{
    ceiling::Contained,
    ipc::{self, Reply, Request},
    ranges::ChunkSource,
};
use anyhow::{Result, bail, ensure};
use std::{
    cell::RefCell,
    io::{Read, Write},
};

/// Serves one decode over stdio; the parent kills this process to cancel it.
pub fn serve(contained: Contained) -> Result<()> {
    ensure!(
        std::env::var(crate::policy::DEVELOPER_ENV).as_deref() == Ok("1"),
        "media helper disabled"
    );
    serve_on(
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &contained,
    )
}

/// Reports any failure to the parent as an error frame before returning it.
pub(crate) fn serve_on(
    input: &mut impl Read,
    output: &mut impl Write,
    contained: &Contained,
) -> Result<()> {
    let output = RefCell::new(output);
    let result = decode_session(input, &output, contained);
    if let Err(error) = &result {
        let _ = ipc::write_reply(*output.borrow_mut(), &Reply::Error(format!("{error:#}")));
    }
    result
}

fn decode_session<W: Write>(
    input: &mut impl Read,
    output: &RefCell<&mut W>,
    contained: &Contained,
) -> Result<()> {
    let Request::Start(start) = ipc::read_request(input)? else {
        bail!("media helper expected a start record");
    };
    start.descriptor.validate_profile()?;
    #[cfg(feature = "developer-media")]
    {
        let reader =
            super::ranges::RangeReader::new(start.descriptor.clone(), IpcChunks { input, output });
        super::webm::decode(
            reader,
            &start.descriptor,
            start.start_us,
            contained,
            |out| ipc::write_reply(*output.borrow_mut(), &Reply::Output(out)),
        )
    }
    #[cfg(not(feature = "developer-media"))]
    {
        let _ = (contained, output);
        bail!("this helper was built without the developer-media decoder")
    }
}

/// Requests chunks from the parent, which alone holds network authority.
#[cfg_attr(not(feature = "developer-media"), allow(dead_code))]
struct IpcChunks<'a, 'b, R, W> {
    input: &'a mut R,
    output: &'a RefCell<&'b mut W>,
}

impl<R: Read, W: Write> ChunkSource for IpcChunks<'_, '_, R, W> {
    fn load(&mut self, index: usize, _start: u64, length: u64) -> Result<Vec<u8>> {
        let index = u32::try_from(index)?;
        ipc::write_reply(*self.output.borrow_mut(), &Reply::Need(index))?;
        let Request::Chunk { index: got, bytes } = ipc::read_request(self.input)? else {
            bail!("media helper expected a chunk");
        };
        ensure!(
            got == index && bytes.len() as u64 == length,
            "media chunk mismatch"
        );
        Ok(bytes)
    }
}
