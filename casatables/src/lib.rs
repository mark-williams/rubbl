// Copyright 2017 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

extern crate rubbl_core;
extern crate rubbl_casatables_impl;

use rubbl_core::Complex;

mod glue;

/// OMG. Strings were incredibly painful.
///
/// One important piece of context: `casacore::String` is a subclass of C++'s
/// `std::string`. Rust strings can contain interior NUL bytes. Fortunately,
/// `std::string` can as well, so we don't need to worry about the C string
/// convention.
///
/// My understanding is that C++'s `std::string` always allocates its own
/// buffer. So we can't try to be clever about lifetimes and borrowing: every
/// time we bridge to C++ there's going to be a copy.
///
/// Then I ran into problems essentially because of the following bindgen
/// problem: https://github.com/rust-lang-nursery/rust-bindgen/issues/778 . On
/// Linux small classes, such as String, have special ABI conventions, and
/// bindgen does not represent them properly to Rust at the moment (Sep 2017).
/// The String class is a victim of this problem, which led to completely
/// bizarre failures in my code when the small-string optimization was kicking
/// in. It seems that if we only interact with the C++ through pointers and
/// references to Strings, things remain OK.
///
/// Finally, as best I understand it, we need to manually ensure that the C++
/// destructor for the String class is run. I have done this with a little
/// trick off of StackExchange.

impl glue::GlueString {
    fn from_rust(s: &str) -> Self {
        unsafe {
            let mut cs = ::std::mem::zeroed::<glue::GlueString>();
            glue::string_init(&mut cs, s.as_ptr() as _, s.len() as u64);
            cs
        }
    }
}

impl Drop for glue::GlueString {
    fn drop(&mut self) {
        unsafe { glue::string_deinit(self) };
    }
}

// Exceptions

impl glue::ExcInfo {
    fn as_error(&self) -> ::std::io::Error {
        let c_str = unsafe { ::std::ffi::CStr::from_ptr(self.message.as_ptr()) };

        let msg = match c_str.to_str() {
            Ok(s) => s,
            Err(_) => "[un-translatable C++ exception]",
        };

        ::std::io::Error::new(::std::io::ErrorKind::Other, msg)
    }

    fn as_err<T>(&self) -> Result<T,::std::io::Error> {
        Err(self.as_error())
    }
}


// Data types

impl glue::GlueDataType {
    fn size(&self) -> i32 {
        unsafe { glue::data_type_get_element_size(*self) as i32 }
    }
}


trait CasaDataType: Sized {
    const DATA_TYPE: glue::GlueDataType;

    #[cfg(test)]
    fn test_casa_data_size() {
        assert_eq!(std::mem::size_of::<Self>() as i32, Self::DATA_TYPE.size());
    }
}


impl CasaDataType for bool {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpBool;
}

impl CasaDataType for i8 {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpChar;
}

impl CasaDataType for u8 {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpUChar;
}

impl CasaDataType for i16 {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpShort;
}

impl CasaDataType for u16 {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpUShort;
}

impl CasaDataType for i32 {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpInt;
}

impl CasaDataType for u32 {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpUInt;
}

impl CasaDataType for i64 {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpInt64;
}

impl CasaDataType for f32 {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpFloat;
}

impl CasaDataType for f64 {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpDouble;
}

impl CasaDataType for Complex<f32> {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpComplex;
}

impl CasaDataType for Complex<f64> {
    const DATA_TYPE: glue::GlueDataType = glue::GlueDataType::TpDComplex;
}


#[cfg(test)]
mod data_type_tests {
    use super::*;

    #[test]
    fn sizes() {
        bool::test_casa_data_size();
        i8::test_casa_data_size();
        u8::test_casa_data_size();
        i16::test_casa_data_size();
        u16::test_casa_data_size();
        i32::test_casa_data_size();
        u32::test_casa_data_size();
        i64::test_casa_data_size();
        f32::test_casa_data_size();
        f64::test_casa_data_size();
        Complex::<f32>::test_casa_data_size();
        Complex::<f64>::test_casa_data_size();
    }
}


// Tables

pub struct Table {
    handle: *mut glue::GlueTable,
    exc_info: glue::ExcInfo,
}

impl Table {
    pub fn open(name: &str) -> Result<Self,::std::io::Error> {
        let cname = glue::GlueString::from_rust(name);
        let mut exc_info = unsafe { ::std::mem::zeroed::<glue::ExcInfo>() };

        let handle = unsafe { glue::table_alloc_and_open(&cname, &mut exc_info) };
        if handle.is_null() {
            return exc_info.as_err();
        }

        Ok(Table {
            handle: handle,
            exc_info: exc_info,
        })
    }

    pub fn n_rows(&self) -> usize {
        unsafe { glue::table_n_rows(self.handle) as usize }
    }

    pub fn deep_copy_no_rows(&mut self, dest_path: &str) -> Result<(),::std::io::Error> {
        let cdest_path = glue::GlueString::from_rust(dest_path);

        if unsafe { glue::table_deep_copy_no_rows(self.handle, &cdest_path, &mut self.exc_info) != 0 } {
            self.exc_info.as_err()
        } else {
            Ok(())
        }
    }
}


impl Drop for Table {
    fn drop(&mut self) {
        // FIXME: not sure if this function can actually produce useful
        // exceptions anyway, but we can't do anything if it does!
        unsafe { glue::table_close_and_free(self.handle, &mut self.exc_info) }
    }
}


#[cfg(test)]
mod tests {
    use super::glue;

    #[test]
    fn check_string_size() {
        let cpp_size = unsafe { glue::string_check_size() } as usize;
        let rust_size = ::std::mem::size_of::<glue::GlueString>();
        assert_eq!(cpp_size, rust_size);
    }
}
