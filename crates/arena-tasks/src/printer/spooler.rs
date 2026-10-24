use arena_proto::FoundPrinter;

use super::PrinterError;

pub const RAW_DATATYPE: &str = "RAW";
pub const TEST_DOC_NAME: &str = "Arena 360 printer test";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDocInfo {
    doc_name: Vec<u16>,
    datatype: Vec<u16>,
}

impl RawDocInfo {
    pub fn new(doc_name: &str) -> Self {
        Self {
            doc_name: wide(doc_name),
            datatype: wide(RAW_DATATYPE),
        }
    }

    pub fn doc_name(&self) -> &[u16] {
        &self.doc_name
    }

    pub fn datatype(&self) -> &[u16] {
        &self.datatype
    }

    #[cfg(windows)]
    fn as_doc_info(&mut self) -> windows::Win32::Graphics::Printing::DOC_INFO_1W {
        windows::Win32::Graphics::Printing::DOC_INFO_1W {
            pDocName: windows::core::PWSTR(self.doc_name.as_mut_ptr()),
            pOutputFile: windows::core::PWSTR::null(),
            pDatatype: windows::core::PWSTR(self.datatype.as_mut_ptr()),
        }
    }
}

#[cfg(not(windows))]
pub(super) fn local_printers() -> Vec<FoundPrinter> {
    Vec::new()
}

#[cfg(not(windows))]
pub(super) fn print_raw(_name: &str, _bytes: &[u8]) -> Result<(), PrinterError> {
    Err(PrinterError::UsbUnavailable)
}

#[cfg(windows)]
pub(super) fn local_printers() -> Vec<FoundPrinter> {
    use windows::Win32::Graphics::Printing::{
        EnumPrintersW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_4W,
    };
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let mut needed = 0u32;
    let mut count = 0u32;
    let _ = unsafe {
        EnumPrintersW(
            flags,
            windows::core::PCWSTR::null(),
            4,
            None,
            &mut needed,
            &mut count,
        )
    };
    if needed == 0 {
        return Vec::new();
    }
    let mut words = vec![0u64; (needed as usize).div_ceil(std::mem::size_of::<u64>())];
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(
            words.as_mut_ptr().cast::<u8>(),
            words.len() * std::mem::size_of::<u64>(),
        )
    };
    let listed = unsafe {
        EnumPrintersW(
            flags,
            windows::core::PCWSTR::null(),
            4,
            Some(bytes),
            &mut needed,
            &mut count,
        )
    };
    if let Err(e) = listed {
        tracing::warn!(detail = %e, "the spooler could not list printers");
        return Vec::new();
    }
    let infos = unsafe {
        std::slice::from_raw_parts(words.as_ptr().cast::<PRINTER_INFO_4W>(), count as usize)
    };
    let mut out: Vec<FoundPrinter> = infos
        .iter()
        .filter(|i| !i.pPrinterName.is_null())
        .filter_map(|i| unsafe { i.pPrinterName.to_string() }.ok())
        .filter(|name| !name.trim().is_empty())
        .map(|name| FoundPrinter {
            id: name.clone(),
            kind: arena_proto::PrinterKind::Usb,
            name,
            addr: None,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

#[cfg(windows)]
struct OpenPrinter(windows::Win32::Graphics::Printing::PRINTER_HANDLE);

#[cfg(windows)]
impl Drop for OpenPrinter {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Graphics::Printing::ClosePrinter(self.0) };
    }
}

#[cfg(windows)]
pub(super) fn print_raw(name: &str, bytes: &[u8]) -> Result<(), PrinterError> {
    use windows::Win32::Graphics::Printing::{
        EndDocPrinter, EndPagePrinter, OpenPrinterW, PRINTER_HANDLE, StartDocPrinterW,
        StartPagePrinter, WritePrinter,
    };
    let fail = |what: &str| {
        PrinterError::Spooler(
            name.to_owned(),
            format!(
                "{what} failed ({})",
                windows::core::Error::from_thread().message()
            ),
        )
    };
    let printer_name = wide(name);
    let mut handle = PRINTER_HANDLE::default();
    unsafe {
        OpenPrinterW(
            windows::core::PCWSTR(printer_name.as_ptr()),
            &mut handle,
            None,
        )
    }
    .map_err(|e| PrinterError::Spooler(name.to_owned(), e.message()))?;
    let printer = OpenPrinter(handle);
    let mut doc = RawDocInfo::new(TEST_DOC_NAME);
    let info = doc.as_doc_info();
    let job = unsafe { StartDocPrinterW(printer.0, 1, &info) };
    if job == 0 {
        return Err(fail("StartDocPrinter"));
    }
    let result = (|| {
        if !unsafe { StartPagePrinter(printer.0) }.as_bool() {
            return Err(fail("StartPagePrinter"));
        }
        let mut written = 0u32;
        let len = u32::try_from(bytes.len()).map_err(|_| fail("WritePrinter"))?;
        let ok = unsafe { WritePrinter(printer.0, bytes.as_ptr().cast(), len, &mut written) };
        if !ok.as_bool() || written != len {
            return Err(fail("WritePrinter"));
        }
        if !unsafe { EndPagePrinter(printer.0) }.as_bool() {
            return Err(fail("EndPagePrinter"));
        }
        Ok(())
    })();
    let ended = unsafe { EndDocPrinter(printer.0) }.as_bool();
    result?;
    if !ended {
        return Err(fail("EndDocPrinter"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_doc_info_names_the_raw_datatype_in_utf16() {
        let doc = RawDocInfo::new(TEST_DOC_NAME);
        assert_eq!(doc.datatype(), &[b'R' as u16, b'A' as u16, b'W' as u16, 0]);
        assert_eq!(doc.doc_name().last(), Some(&0));
        assert_eq!(
            String::from_utf16(&doc.doc_name()[..doc.doc_name().len() - 1]).unwrap(),
            TEST_DOC_NAME
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_doc_info_points_at_the_raw_strings_and_no_file() {
        let mut doc = RawDocInfo::new(TEST_DOC_NAME);
        let info = doc.as_doc_info();
        assert!(info.pOutputFile.is_null());
        assert_eq!(unsafe { info.pDatatype.to_string() }.unwrap(), "RAW");
        assert_eq!(unsafe { info.pDocName.to_string() }.unwrap(), TEST_DOC_NAME);
    }
}
