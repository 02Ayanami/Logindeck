//! Lazy, bounded icons. Failed resources never affect catalog discovery.
use crate::{
    app_catalog::{package, win32, IconSource},
    WindowsLaunchTarget,
};
use autologin_core::ApplicationRecord;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::Path;

const MAX_DIMENSION: u32 = 512;
const MAX_PIXELS_BYTES: usize = 512 * 512 * 4;
const MAX_PNG_BYTES: usize = MAX_PIXELS_BYTES + 64 * 1024;
const MAX_URL_BYTES: usize = 1500 * 1024;

/// Compatibility entry point for a local executable, DLL, or ICO resource.
pub fn application_icon(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        native::resource_icon(path, 0)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Resolves a stored typed target lazily. Call from a blocking worker.
/// Current metadata is checked before using registration resources; missing icons are benign.
pub fn application_icon_for_record(app: &ApplicationRecord) -> Option<String> {
    let target = crate::launch::validated_target(app).ok()?;
    #[cfg(windows)]
    match target {
        WindowsLaunchTarget::Executable(path) => {
            let current = win32::filesystem::executable(path.to_str()?)?;
            if current.fingerprint != app.signature_identity {
                return None;
            }
            let sources = win32::Win32Inventory.enumerate().ok().unwrap_or_default()
                .into_iter().find(|candidate| {
                    (candidate.identity == app.platform_application_id || candidate.alternate_identities.contains(&app.platform_application_id))
                        && candidate.signature_identity == app.signature_identity
                        && matches!(&candidate.target, WindowsLaunchTarget::Executable(p) if win32::path_key(p) == win32::path_key(&current.path))
                }).map(|candidate| candidate.icon_sources).unwrap_or_default();
            from_sources(&current.path, &sources, native::resource_icon)
        }
        WindowsLaunchTarget::Aumid(aumid) => {
            package::with_logo(&aumid, &app.signature_identity, |reference| {
                let stream = reference.OpenReadAsync().ok()?.get().ok()?;
                native::logo_stream(&stream)
            })
        }
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        None
    }
}

fn from_sources(
    path: &Path,
    sources: &[IconSource],
    mut extract: impl FnMut(&Path, i32) -> Option<String>,
) -> Option<String> {
    sources
        .iter()
        .take(16)
        .find_map(|source| extract(&source.path, source.index))
        .or_else(|| extract(path, 0))
}

fn pixel_len(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return None;
    }
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
        .filter(|length| *length <= MAX_PIXELS_BYTES)
}

fn png_url(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    if pixel_len(width, height)? != rgba.len() {
        return None;
    }
    // Bounded writer rejects excess compressed output during encoding, not after allocation.
    struct Output(Vec<u8>);
    impl std::io::Write for Output {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) > MAX_PNG_BYTES {
                return Err(std::io::ErrorKind::FileTooLarge.into());
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Output(Vec::new());
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
        writer.finish().ok()?;
    }
    let size = 22usize.checked_add(output.0.len().div_ceil(3).checked_mul(4)?)?;
    if size > MAX_URL_BYTES {
        return None;
    }
    Some(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(output.0)
    ))
}

#[cfg(windows)]
mod native {
    use super::*;
    use crate::app_catalog::win32::{filesystem::checked_file, local_path};
    use windows::{
        core::PCWSTR,
        Win32::{
            Graphics::{
                Gdi::{DeleteObject, GetObjectW, BITMAP, HBITMAP},
                Imaging::{
                    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory,
                    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
                },
            },
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_MULTITHREADED,
            },
            UI::{
                Shell::ExtractIconExW,
                WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO},
            },
        },
    };

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    struct Icon(HICON);
    impl Drop for Icon {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                let _ = unsafe { DestroyIcon(self.0) };
            }
        }
    }
    struct Bitmap(HBITMAP);
    impl Drop for Bitmap {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                let _ = unsafe { DeleteObject(self.0.into()) };
            }
        }
    }

    pub(super) fn logo_stream(stream: &impl windows::core::Interface) -> Option<String> {
        use windows::{
            Graphics::Imaging::{
                BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform,
                ColorManagementMode, ExifOrientationMode,
            },
            Storage::Streams::IRandomAccessStream,
        };
        // Borrow an interface clone for the duration; all COM/WinRT objects own their references.
        let stream = stream.cast::<IRandomAccessStream>().ok()?;
        if stream.Size().ok()? == 0 || stream.Size().ok()? > 4 * 1024 * 1024 {
            return None;
        }
        let decoder = BitmapDecoder::CreateAsync(&stream).ok()?.get().ok()?;
        let width = decoder.PixelWidth().ok()?;
        let height = decoder.PixelHeight().ok()?;
        let expected = pixel_len(width, height)?;
        let transform = BitmapTransform::new().ok()?;
        let pixels = decoder
            .GetPixelDataTransformedAsync(
                BitmapPixelFormat::Rgba8,
                BitmapAlphaMode::Straight,
                &transform,
                ExifOrientationMode::IgnoreExifOrientation,
                ColorManagementMode::DoNotColorManage,
            )
            .ok()?
            .get()
            .ok()?;
        let bytes = pixels.DetachPixelData().ok()?;
        if bytes.len() != expected {
            return None;
        }
        png_url(width, height, &bytes)
    }

    pub(super) fn resource_icon(path: &Path, index: i32) -> Option<String> {
        let value = path.to_str().filter(|value| local_path(value))?;
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        if !matches!(extension.as_str(), "exe" | "dll" | "ico") {
            return None;
        }
        let pinned = checked_file(value)?;
        let limit = if extension == "ico" {
            4 * 1024 * 1024
        } else {
            256 * 1024 * 1024
        };
        if pinned.file_size() == 0 || pinned.file_size() > limit {
            return None;
        }
        // A caller may already have an STA apartment. WIC works in either model;
        // only successful initialization is paired with uninitialization.
        let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let _apartment = if status.is_ok() {
            Some(Apartment)
        } else if status.0 == 0x80010106u32 as i32 {
            None
        } else {
            return None;
        };
        let wide: Vec<_> = pinned.path.to_str()?.encode_utf16().chain([0]).collect();
        let mut icon = Icon(HICON::default());
        let count =
            unsafe { ExtractIconExW(PCWSTR(wide.as_ptr()), index, Some(&mut icon.0), None, 1) };
        if count != 1 || icon.0.is_invalid() {
            return None;
        }
        let mut info = ICONINFO::default();
        let info_result = unsafe { GetIconInfo(icon.0, &mut info) };
        let color = Bitmap(info.hbmColor);
        let mask = Bitmap(info.hbmMask);
        info_result.ok()?;
        let bitmap = if color.0.is_invalid() { &mask } else { &color };
        let mut properties = BITMAP::default();
        if unsafe {
            GetObjectW(
                bitmap.0.into(),
                std::mem::size_of::<BITMAP>() as i32,
                Some((&mut properties as *mut BITMAP).cast()),
            )
        } == 0
        {
            return None;
        }
        let height = if color.0.is_invalid() {
            properties.bmHeight / 2
        } else {
            properties.bmHeight
        };
        pixel_len(
            u32::try_from(properties.bmWidth).ok()?,
            u32::try_from(height).ok()?,
        )?;
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .ok()?;
        let bitmap = unsafe { factory.CreateBitmapFromHICON(icon.0) }.ok()?;
        let (mut width, mut height) = (0, 0);
        unsafe { bitmap.GetSize(&mut width, &mut height) }.ok()?;
        let mut pixels = vec![0; pixel_len(width, height)?];
        let converter = unsafe { factory.CreateFormatConverter() }.ok()?;
        unsafe {
            converter.Initialize(
                &bitmap,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
        }
        .ok()?;
        unsafe { converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels) }.ok()?;
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        png_url(width, height, &pixels)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::{fs, path::PathBuf};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("logindeck-icons-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn icon(&self) -> PathBuf {
            // Independent 16x16 ICO: BITMAPINFOHEADER, opaque red BGRA, zero AND mask.
            let mut bytes = vec![0, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 32, 0];
            bytes.extend(1128u32.to_le_bytes());
            bytes.extend(22u32.to_le_bytes());
            for value in [40u32, 16, 32] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend([1, 0, 32, 0]);
            for value in [0u32, 1024, 0, 0, 0, 0] {
                bytes.extend(value.to_le_bytes());
            }
            for _ in 0..256 {
                bytes.extend([0, 0, 255, 255]);
            }
            bytes.extend([0u8; 64]);
            let path = self.0.join("fixture.ico");
            fs::write(&path, bytes).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn controlled_icon_is_png_and_native_resources_are_released() {
        let fixture = Fixture::new();
        let path = fixture.icon();
        let first = application_icon(&path).expect("controlled ICO must decode");
        assert!(first.starts_with("data:image/png;base64,iVBORw0KGgo"));
        let bytes = STANDARD
            .decode(first.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let mut decoded = png::Decoder::new(std::io::Cursor::new(&bytes))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; decoded.output_buffer_size()];
        let info = decoded.next_frame(&mut pixels).unwrap();
        let colors: std::collections::BTreeSet<_> =
            pixels[..info.buffer_size()].chunks_exact(4).collect();
        // ExtractIconEx scales this 16px resource to the system large-icon size;
        // Windows interpolation may round red/alpha down by one at the boundary.
        assert!(colors
            .iter()
            .all(|rgba| rgba[0] >= 254 && rgba[1] == 0 && rgba[2] == 0 && rgba[3] >= 254));
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS, GR_USEROBJECTS,
        };
        let resources = || unsafe {
            (
                GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS),
                GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS),
            )
        };
        let before = resources();
        for _ in 0..100 {
            assert_eq!(application_icon(&path), Some(first.clone()));
        }
        assert_eq!(
            resources(),
            before,
            "HICON/HBITMAP counts must remain stable"
        );
        fs::rename(&path, fixture.0.join("released.ico")).unwrap();
    }

    #[test]
    fn icon_missing_malformed_oversized_and_unsafe_inputs_are_none() {
        let fixture = Fixture::new();
        let path = fixture.0.join("broken.ico");
        fs::write(&path, b"not an icon").unwrap();
        assert!(application_icon(&path).is_none());
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(4 * 1024 * 1024 + 1)
            .unwrap();
        assert!(application_icon(&path).is_none());
        for path in [
            "relative.ico",
            r"C:\missing-logindeck.ico",
            r"\\server\icon.ico",
            "aumid:Contoso_abc!App",
        ] {
            assert!(application_icon(std::path::Path::new(path)).is_none());
        }
        assert!(png_url(0, 1, &[]).is_none());
        assert!(png_url(513, 1, &vec![0; 513 * 4]).is_none());
        assert!(png_url(2, 2, &[0; 15]).is_none());
    }

    #[test]
    fn target_icon_prefers_indexed_resource_then_executable_fallback() {
        use crate::app_catalog::IconSource;
        let executable = Path::new(r"C:\Fixture\App.exe");
        let sources = vec![IconSource {
            path: r"C:\Fixture\Brand.dll".into(),
            index: -17,
        }];
        let mut calls = vec![];
        let result = from_sources(executable, &sources, |path, index| {
            calls.push((path.to_owned(), index));
            Some("resource".into())
        });
        assert_eq!(result.as_deref(), Some("resource"));
        assert_eq!(calls, vec![(PathBuf::from(r"C:\Fixture\Brand.dll"), -17)]);
        calls.clear();
        let result = from_sources(executable, &sources, |path, index| {
            calls.push((path.to_owned(), index));
            (path == executable).then(|| "fallback".into())
        });
        assert_eq!(result.as_deref(), Some("fallback"));
        assert_eq!(
            calls,
            vec![
                (PathBuf::from(r"C:\Fixture\Brand.dll"), -17),
                (executable.to_owned(), 0)
            ]
        );
    }

    #[test]
    fn package_logo_stream_decodes_bounded_pixels_and_rejects_bombs() {
        use windows::{
            Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
            Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
        };
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe { RoUninitialize() };
            }
        }
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.unwrap();
        let _apartment = Apartment;
        let stream = InMemoryRandomAccessStream::new().unwrap();
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[255, 0, 0, 255, 0, 255, 0, 255])
                .unwrap();
        }
        let writer = DataWriter::CreateDataWriter(&stream).unwrap();
        writer.WriteBytes(&png).unwrap();
        writer.StoreAsync().unwrap().get().unwrap();
        stream.Seek(0).unwrap();
        let output = native::logo_stream(&stream).unwrap();
        assert!(output.starts_with("data:image/png;base64,iVBORw0KGgo"));
        stream.SetSize(4 * 1024 * 1024 + 1).unwrap();
        assert!(native::logo_stream(&stream).is_none());
        let bad = InMemoryRandomAccessStream::new().unwrap();
        let writer = DataWriter::CreateDataWriter(&bad).unwrap();
        writer.WriteBytes(b"malformed").unwrap();
        writer.StoreAsync().unwrap().get().unwrap();
        bad.Seek(0).unwrap();
        assert!(native::logo_stream(&bad).is_none());
        let large = InMemoryRandomAccessStream::new().unwrap();
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 513, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![255; 513 * 4])
                .unwrap();
        }
        let writer = DataWriter::CreateDataWriter(&large).unwrap();
        writer.WriteBytes(&png).unwrap();
        writer.StoreAsync().unwrap().get().unwrap();
        large.Seek(0).unwrap();
        assert!(
            native::logo_stream(&large).is_none(),
            "dimension limit must precede pixel decoding"
        );
    }
}
