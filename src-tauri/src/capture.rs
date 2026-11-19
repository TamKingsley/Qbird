//! Full-screen capture via GDI, returned as a BMP data URL.
//!
//! The front end shows the capture in a selection overlay, crops the chosen
//! rectangle on a canvas and sends the result back as a normal image file. BMP is
//! used because it needs no encoder dependency; the bytes stay local.
#[cfg(windows)]
use std::ptr::null_mut;

pub struct ScreenCapture {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
pub fn capture() -> Result<ScreenCapture, String> {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    // SAFETY: every GDI handle created below is released on all paths before return.
    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        if width <= 0 || height <= 0 || width > 16_384 || height > 16_384 {
            return Err("无法读取屏幕尺寸".into());
        }
        let screen = GetDC(null_mut());
        if screen.is_null() { return Err("无法访问屏幕".into()); }
        let memory = CreateCompatibleDC(screen);
        if memory.is_null() { ReleaseDC(null_mut(), screen); return Err("无法创建截图缓冲区".into()); }
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        if bitmap.is_null() { DeleteDC(memory); ReleaseDC(null_mut(), screen); return Err("无法创建截图位图".into()); }
        let previous = SelectObject(memory, bitmap);
        let copied = BitBlt(memory, 0, 0, width, height, screen, 0, 0, SRCCOPY);
        if copied == 0 {
            SelectObject(memory, previous);
            DeleteObject(bitmap);
            DeleteDC(memory);
            ReleaseDC(null_mut(), screen);
            return Err("截屏失败".into());
        }
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        let lines = GetDIBits(memory, bitmap, 0, height as u32, pixels.as_mut_ptr() as *mut _, &mut info, DIB_RGB_COLORS);
        SelectObject(memory, previous);
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(null_mut(), screen);
        if lines == 0 || lines as i32 != height { return Err("无法读取截图像素".into()); }
        let bmp = encode_bmp(&pixels, width as u32, height as u32);
        Ok(ScreenCapture {
            data_url: format!("data:image/bmp;base64,{}", crate::files::base64_encode(&bmp)),
            width: width as u32,
            height: height as u32,
        })
    }
}

#[cfg(not(windows))]
pub fn capture() -> Result<ScreenCapture, String> {
    Err("当前平台暂不支持截图".into())
}

/// 32-bit bottom-up BMP with a 54-byte header; pixel order from GetDIBits is BGRA.
pub fn encode_bmp(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let image_size = (width as usize) * (height as usize) * 4;
    let file_size = 54 + image_size;
    let mut data = Vec::with_capacity(file_size);
    data.extend_from_slice(b"BM");
    data.extend_from_slice(&(file_size as u32).to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&54u32.to_le_bytes());
    data.extend_from_slice(&40u32.to_le_bytes());
    data.extend_from_slice(&(width as i32).to_le_bytes());
    data.extend_from_slice(&(height as i32).to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&32u16.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&(image_size as u32).to_le_bytes());
    data.extend_from_slice(&[0u8; 16]);
    data.extend_from_slice(&pixels[..image_size.min(pixels.len())]);
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_header_matches_pixel_payload() {
        let pixels = vec![0u8; 4 * 2 * 3];
        let bmp = encode_bmp(&pixels, 2, 3);
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(bmp.len(), 54 + 24);
        assert_eq!(u32::from_le_bytes([bmp[2], bmp[3], bmp[4], bmp[5]]), 78);
        assert_eq!(i32::from_le_bytes([bmp[18], bmp[19], bmp[20], bmp[21]]), 2);
        assert_eq!(i32::from_le_bytes([bmp[22], bmp[23], bmp[24], bmp[25]]), 3);
    }
}
