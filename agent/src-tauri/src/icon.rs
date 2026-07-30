use base64::Engine;
use core::ffi::c_void;
use std::ffi::OsStr;
use std::io::Cursor;
use std::mem::{self, MaybeUninit};
use std::os::windows::ffi::OsStrExt;

use image::RgbaImage;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HGDIOBJ, ReleaseDC,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Controls::IImageList;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    SHFILEINFOW, SHGFI_PIDL, SHGFI_SYSICONINDEX, SHGetFileInfoW, SHGetImageList, SHParseDisplayName,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON};

const SHIL_JUMBO: i32 = 0x4;
const ILD_TRANSPARENT: u32 = 0x1;

pub fn extract_icon_data_url(exe_path: &str) -> Option<String> {
    let img =
        extract_jumbo_icon(exe_path).or_else(|| windows_icons::get_icon_by_path(exe_path).ok())?;
    image_to_data_url(img)
}

/// 셸 네임스페이스 위치(예: "shell:RecycleBinFolder")의 아이콘. 파일 경로가 없는
/// 특수 폴더용 — SHParseDisplayName으로 PIDL을 얻어 시스템 아이콘을 뽑는다.
pub fn extract_icon_data_url_shell(shell_name: &str) -> Option<String> {
    let hicon = unsafe { get_jumbo_hicon_shell(shell_name) }?;
    let img = unsafe { hicon_to_image(hicon) };
    unsafe {
        let _ = DestroyIcon(hicon);
    }
    image_to_data_url(img?)
}

// 일부 exe는 256px 아이콘 리소스가 없어서, 점보 이미지 리스트가 그보다 작은
// 원본 아이콘(예: 48x48)을 확대하지 않고 256x256 캔버스의 좌상단에 그대로
// 박아넣는다 - 실측 결과 밤부스튜디오가 정확히 이 경우였다(canvas 256x256,
// 실제 내용물은 (0,0)-(47,47)). 내용물의 알파 바운딩박스를 찾아 잘라낸 뒤
// 캔버스를 다시 채우도록 확대·중앙 배치해 보정한다.
fn normalize_icon(img: RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut found = false;
    for (x, y, px) in img.enumerate_pixels() {
        if px[3] > 8 {
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if !found {
        return img;
    }
    let content_w = max_x - min_x + 1;
    let content_h = max_y - min_y + 1;
    // 내용물이 이미 캔버스 대부분을 채우고 있으면(정상 케이스) 손대지 않는다.
    let fill_ratio = content_w.max(content_h) as f32 / w.max(h) as f32;
    if fill_ratio > 0.85 {
        return img;
    }
    let cropped = image::imageops::crop_imm(&img, min_x, min_y, content_w, content_h).to_image();
    let target = (w.min(h) as f32 * 0.92) as u32;
    let scale = target as f32 / content_w.max(content_h) as f32;
    let new_w = ((content_w as f32) * scale).round().max(1.0) as u32;
    let new_h = ((content_h as f32) * scale).round().max(1.0) as u32;
    let resized =
        image::imageops::resize(&cropped, new_w, new_h, image::imageops::FilterType::CatmullRom);
    let mut canvas = RgbaImage::new(w, h);
    let off_x = ((w - new_w) / 2) as i64;
    let off_y = ((h - new_h) / 2) as i64;
    image::imageops::overlay(&mut canvas, &resized, off_x, off_y);
    canvas
}

fn image_to_data_url(img: RgbaImage) -> Option<String> {
    let img = normalize_icon(img);
    let dynamic = image::DynamicImage::ImageRgba8(img);
    let mut bytes: Vec<u8> = Vec::new();
    dynamic
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:image/png;base64,{encoded}"))
}

/// SHFILEINFOW의 시스템 아이콘 인덱스에서 점보(256px) HICON을 얻는다.
unsafe fn jumbo_from_icon_index(icon_index: i32) -> Option<HICON> {
    let image_list: Option<IImageList> = unsafe { SHGetImageList(SHIL_JUMBO) }.ok();
    image_list.and_then(|list| unsafe { list.GetIcon(icon_index, ILD_TRANSPARENT) }.ok())
}

unsafe fn get_jumbo_hicon_shell(shell_name: &str) -> Option<HICON> {
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

    let wide: Vec<u16> = OsStr::new(shell_name)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
    let parsed = unsafe {
        SHParseDisplayName(
            PCWSTR::from_raw(wide.as_ptr()),
            None,
            &mut pidl,
            0,
            None,
        )
    };
    if parsed.is_err() || pidl.is_null() {
        unsafe { CoUninitialize() };
        return None;
    }

    let mut info = MaybeUninit::<SHFILEINFOW>::uninit();
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR::from_raw(pidl as *const u16),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(info.as_mut_ptr()),
            mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_PIDL | SHGFI_SYSICONINDEX,
        )
    };
    unsafe { CoTaskMemFree(Some(pidl as *const c_void)) };

    if result == 0 {
        unsafe { CoUninitialize() };
        return None;
    }
    let info = unsafe { info.assume_init() };
    let hicon = unsafe { jumbo_from_icon_index(info.iIcon) };
    unsafe { CoUninitialize() };
    hicon
}

fn extract_jumbo_icon(path: &str) -> Option<RgbaImage> {
    let hicon = unsafe { get_jumbo_hicon(path) }?;
    let image = unsafe { hicon_to_image(hicon) };
    unsafe {
        let _ = DestroyIcon(hicon);
    }
    image
}

unsafe fn get_jumbo_hicon(path: &str) -> Option<HICON> {
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

    let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
    let mut info = MaybeUninit::<SHFILEINFOW>::uninit();
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR::from_raw(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(info.as_mut_ptr()),
            mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX,
        )
    };
    if result == 0 {
        unsafe { CoUninitialize() };
        return None;
    }
    let info = unsafe { info.assume_init() };

    let hicon = unsafe { jumbo_from_icon_index(info.iIcon) };

    unsafe { CoUninitialize() };
    hicon
}

unsafe fn hicon_to_image(icon: HICON) -> Option<RgbaImage> {
    let mut info = MaybeUninit::uninit();
    unsafe { GetIconInfo(icon, info.as_mut_ptr()) }.ok()?;
    let info = unsafe { info.assume_init() };

    let mut bitmap: MaybeUninit<BITMAP> = MaybeUninit::uninit();
    let bitmap_size = mem::size_of::<BITMAP>() as i32;
    let result = unsafe {
        GetObjectW(
            HGDIOBJ::from(info.hbmColor),
            bitmap_size,
            Some(bitmap.as_mut_ptr().cast()),
        )
    };
    if result != bitmap_size {
        unsafe {
            let _ = DeleteObject(HGDIOBJ::from(info.hbmMask));
            let _ = DeleteObject(HGDIOBJ::from(info.hbmColor));
        }
        return None;
    }
    let bitmap = unsafe { bitmap.assume_init() };

    let width = bitmap.bmWidth.unsigned_abs();
    let height = bitmap.bmHeight.unsigned_abs();
    let mut buf = vec![0u32; (width as usize) * (height as usize)];

    let dc = unsafe { GetDC(None) };
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: bitmap.bmWidth,
            biHeight: -bitmap.bmHeight,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [Default::default()],
    };
    let lines = unsafe {
        GetDIBits(
            dc,
            info.hbmColor,
            0,
            height,
            Some(buf.as_mut_ptr().cast()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = ReleaseDC(None, dc);
        let _ = DeleteObject(HGDIOBJ::from(info.hbmMask));
        let _ = DeleteObject(HGDIOBJ::from(info.hbmColor));
    }

    if lines == 0 {
        return None;
    }

    let pixel_data =
        unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 4) };
    let rgba: Vec<u8> = pixel_data
        .chunks_exact(4)
        .flat_map(|px| [px[2], px[1], px[0], px[3]])
        .collect();
    RgbaImage::from_raw(width, height, rgba)
}

pub fn label_from_path(exe_path: &str) -> String {
    std::path::Path::new(exe_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("app")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 밤부스튜디오 아이콘이 실제로 캔버스 전체를 채우는지, 아니면 작은 내용물이
    // 한쪽 구석에 몰려있는지 실측해서 "아이콘이 작게 한구석에 있다" 버그를 확인한다.
    #[test]
    fn bambu_studio_icon_bounding_box() {
        let path = r"C:\Program Files\Bambu Studio\bambu-studio.exe";
        let img = extract_jumbo_icon(path).expect("failed to extract icon");
        let (w, h) = img.dimensions();
        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        for (x, y, px) in img.enumerate_pixels() {
            if px[3] > 8 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        println!(
            "canvas={w}x{h} content_bbox=({min_x},{min_y})-({max_x},{max_y}) content_w={} content_h={}",
            max_x.saturating_sub(min_x) + 1,
            max_y.saturating_sub(min_y) + 1
        );

        let normalized = normalize_icon(img);
        let (nw, nh) = normalized.dimensions();
        let mut nmin_x = nw;
        let mut nmin_y = nh;
        let mut nmax_x = 0u32;
        let mut nmax_y = 0u32;
        for (x, y, px) in normalized.enumerate_pixels() {
            if px[3] > 8 {
                nmin_x = nmin_x.min(x);
                nmin_y = nmin_y.min(y);
                nmax_x = nmax_x.max(x);
                nmax_y = nmax_y.max(y);
            }
        }
        let ncw = nmax_x.saturating_sub(nmin_x) + 1;
        let nch = nmax_y.saturating_sub(nmin_y) + 1;
        println!(
            "after normalize: canvas={nw}x{nh} content_bbox=({nmin_x},{nmin_y})-({nmax_x},{nmax_y}) content_w={ncw} content_h={nch}"
        );
        assert!(
            ncw.max(nch) as f32 / nw.max(nh) as f32 > 0.85,
            "정규화 후에도 내용물이 캔버스를 채우지 못함"
        );
        // 중앙 정렬 확인: 좌우/상하 여백이 비슷해야 한다(±2px 오차 허용).
        let left_margin = nmin_x;
        let right_margin = nw - 1 - nmax_x;
        assert!(
            left_margin.abs_diff(right_margin) <= 2,
            "가로 중앙 정렬 안됨: left={left_margin} right={right_margin}"
        );
    }

    // 특수 위치 셸 이름들이 실제로 아이콘을 뽑아내는지 실측한다.
    #[test]
    fn shell_locations_resolve_icons() {
        let names = [
            "shell:MyComputerFolder",
            "shell:RecycleBinFolder",
            "shell:Downloads",
            "shell:Desktop",
            "shell:Personal",
            "shell:ControlPanelFolder",
            "shell:NetworkPlacesFolder",
        ];
        for name in names {
            let icon = extract_icon_data_url_shell(name);
            println!(
                "{name}: {}",
                icon.as_ref()
                    .map(|d| format!("ok ({} bytes)", d.len()))
                    .unwrap_or_else(|| "NONE".to_string())
            );
            assert!(icon.is_some(), "{name} 아이콘 추출 실패");
        }
    }
}
