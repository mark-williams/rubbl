// Copyright 2017 Peter Williams
// Licensed under the MIT License.

/*!

Basic I/O helpers.

 */

use byteorder::{BigEndian, ByteOrder};
use num_complex::Complex;
use std::io;
use std::io::Read;

use errors::{ErrorKind, Result};


/// This struct wraps a Read type to equip it with hooks to track its
/// alignment — that is, how many bytes into the stream the read has
/// progressed, and whether the current offset is an exact multiple of a
/// certain number of bytes from the beginning.
///
/// Streams often have alignment requirements so that they can safely be
/// mapped into in-memory data structures. In particular, this is the case for
/// MIRIAD files.
#[derive(Debug)]
pub struct AligningReader<R: Read> {
    inner: R,
    offset: u64
}


impl<R: Read> AligningReader<R> {
    /// Create a new AligningReader that wraps the argument *inner*.
    pub fn new(inner: R) -> Self {
        AligningReader {
            inner: inner,
            offset: 0,
        }
    }

    /// Consume this struct, returning the underlying inner reader.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Return how many bytes we have read since this struct was created.
    ///
    /// Note that this offset is tracked internally. If you open a file, raed
    /// part of it, and *then* create an AligningReader, the returned offset
    /// will refer to the number of bytes read since creation, not the actual
    /// file position as understood by the underlying OS.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Read and discard bytes to ensure that the stream is aligned as specified.
    ///
    /// The maximum allowed alignment value is 64 bytes.
    ///
    /// Returns whether the stream was already at the right alignment. When
    /// that is the case, no read is performed.
    pub fn align_to(&mut self, alignment: usize) -> Result<bool> {
        let mut buf = [0u8; 64];

        if alignment > 64 {
            panic!("maximum alignment size is 64");
        }

        let excess = (self.offset % alignment as u64) as usize;

        if excess == 0 {
            Ok(true)
        } else {
            let amount = alignment - excess;
            let result = self.inner.eof_read_exact(&mut buf[..amount]);

            if result.is_ok() {
                self.offset += amount as u64;
            }

            result
        }
    }
}

impl<R: Read> Read for AligningReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let result = self.inner.read(buf);

        if let Ok(n) = result {
            self.offset += n as u64;
        }

        result
    }
}


/// This is an extension trait that makes it more convenient to handle errors
/// when opening files that may be missing.
///
/// Various parts of Rubbl try to open files but don’t mind if the files are
/// missing. This is expressed using return types like `Result<Option<File>>`:
/// if the value is `Ok(None)`, that means that the file was not found, even
/// though the underlying I/O operation probably return an `Err` type.
///
/// There are times when we want to use these APIs, but it actually is an
/// error if the file in question is missing. This trait provides a
/// `require_found` method on the `Result<Option<T>>` type that removes the
/// `Option` layer of the type, converting `Ok(None)` into an `Err` containing
/// a `NotFound` error.
pub trait OpenResultExt {
    /// The output type of the `require_found` method.
    ///
    /// `Result<Option<T>>` becomes `Result<T>`. Due to the way the trait is
    /// specified, we have to use an associated type to express this fact.
    type Reprocessed;

    /// If *self* is `Ok(None)`, convert it into an `Err` with a `NotFound`
    /// type.
    fn require_found(self) -> Self::Reprocessed;
}


impl<T> OpenResultExt for Result<Option<T>> {
    type Reprocessed = Result<T>;

    fn require_found(self) -> Self::Reprocessed {
        match self {
            Err(e) => Err(e),
            Ok(o) => {
                if let Some(x) = o {
                    Ok(x)
                } else {
                    Err(ErrorKind::Io(io::Error::new(io::ErrorKind::NotFound, "not found")).into())
                }
            }
        }
    }
}


/// Extend the `Read` trait to provide functions for reading an exact number
/// of bytes from a stream and distinguishing whether EOF was encountered
/// immediately, versus whether it was encountered in the midst of the read.
pub trait EofReadExactExt: Read {
    /// Like `Read::read_exact`, except returns Ok(false) if EOF was
    /// encountered at the first read attempt. Returns Ok(true) if everything
    /// was OK and EOF has not yet been hit. Returns Err with an IoError with
    /// a "kind" of UnexpectedEof if EOF was encountered somewhere in the
    /// midst of the buffer.
    fn eof_read_exact(&mut self, buf: &mut [u8]) -> Result<bool>;

    /// Like `byteorder::ReadBytesExt::read_i64::<BigEndian>`, except returns
    /// Some(n) on success and None if EOF was encountered at the first read
    /// attempt.
    fn eof_read_be_i64(&mut self) -> Result<Option<i64>> {
        let mut buf = [0u8; 8];

        if self.eof_read_exact(&mut buf)? {
            Ok(Some(BigEndian::read_i64(&buf)))
        } else {
            Ok(None)
        }
    }

    /// Like `byteorder::ReadBytesExt::read_f32::<BigEndian>`, except returns
    /// Some(n) on success and None if EOF was encountered at the first read
    /// attempt.
    fn eof_read_be_f32(&mut self) -> Result<Option<f32>> {
        let mut buf = [0u8; 4];

        if self.eof_read_exact(&mut buf)? {
            Ok(Some(BigEndian::read_f32(&buf)))
        } else {
            Ok(None)
        }
    }

    /// Like `byteorder::ReadBytesExt::read_f32::<BigEndian>`, except it reads
    /// two values and packs them into a `Complex<f32>`, and returns Some(n)
    /// on success and None if EOF was encountered at the first read attempt.
    /// The real part comes before the imaginary part.
    fn eof_read_be_c64(&mut self) -> Result<Option<Complex<f32>>> {
        let mut buf = [0u8; 8];

        if self.eof_read_exact(&mut buf)? {
            Ok(Some(Complex::new(
                BigEndian::read_f32(&buf[..4]),
                BigEndian::read_f32(&buf[4..])
            )))
        } else {
            Ok(None)
        }
    }
}


impl<R: Read> EofReadExactExt for R {
    fn eof_read_exact(&mut self, buf: &mut [u8]) -> Result<bool> {
        let mut n_left = buf.len();
        let mut ofs = 0;

        while n_left > 0 {
            let n_read = match self.read(&mut buf[ofs..]) {
                Ok(n) => n,
                Err(e) => {
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }

                    return Err(e.into());
                }
            };

            if n_read == 0 {
                return if ofs == 0 {
                    Ok(false) // no more data at an expected stopping point
                } else {
                    Err(ErrorKind::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected EOF")).into())
                };
            }

            ofs += n_read;
            n_left -= n_read;
        }

        Ok(true) // more data, we think
    }
}
