use std::path::Path;

/// Resolves the system icon without launching the application or returning filesystem access.
/// Call on the macOS main thread. Missing bundles and unavailable icons are ordinary fallbacks.
pub fn application_icon(path: &Path) -> Option<String> {
    if !path.is_absolute() || path.extension()? != "app" || !path.is_dir() {
        return None;
    }
    native_icon(path)
}

#[cfg(not(target_os = "macos"))]
fn native_icon(_: &Path) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn native_icon(path: &Path) -> Option<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use objc2::{rc::autoreleasepool, AnyThread, MainThreadMarker};
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
        NSGraphicsContext, NSWorkspace,
    };
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

    MainThreadMarker::new()?;
    autoreleasepool(|_| {
        let image = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path.to_str()?));
        // Fixed pixel dimensions keep IPC and memory bounded; transparency is retained.
        let bitmap = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(), std::ptr::null_mut(), 96, 96, 8, 4, true, false,
                NSDeviceRGBColorSpace, 96 * 4, 32,
            )?
        };
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        image.drawInRect_fromRect_operation_fraction(
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(96., 96.)),
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.)),
            NSCompositingOperation::Copy,
            1.,
        );
        NSGraphicsContext::restoreGraphicsState_class();
        let data = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )?
        };
        let bytes = data.to_vec();
        if bytes.len() > 64 * 1024 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return None;
        }
        Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_and_missing_paths_have_no_icon() {
        for path in ["/bin/sh", "relative.app", "/nonexistent/application.app"] {
            assert!(application_icon(Path::new(path)).is_none());
        }
    }
}
