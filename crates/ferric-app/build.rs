use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let assets_dir = manifest_dir.parent().unwrap().parent().unwrap().join("assets");
    let _ = fs::create_dir_all(&assets_dir);
    let ico_path = assets_dir.join("ferricdaw.ico");
    if !ico_path.exists() {
        let _ = write_ferric_ico(&ico_path);
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" {
        embed_windows_resources(&ico_path, &target_env);
    }
}

/// Generates a crisp 64×64 32-bit BGRA Windows `.ico` file for FerricDAW!
fn write_ferric_ico(path: &Path) -> std::io::Result<()> {
    let w = 64u32;
    let h = 64u32;
    let cx = 31.5_f32;
    let cy = 31.5_f32;

    // DIB XOR mask (bottom-up BGRA) + 1-bit AND mask (padded to 32-bit DWORDs = 8 bytes/row)
    let mut xor_bgra = vec![0u8; (w * h * 4) as usize];
    let and_mask = vec![0u8; (h * 8) as usize];

    for y in 0..h {
        // Bottom-up row index for DIB
        let src_y = (h - 1 - y) as f32;
        for x in 0..w {
            let fx = x as f32;
            let dx = fx - cx;
            let dy = src_y - cy;
            let r = (dx * dx + dy * dy).sqrt();

            let bx = (dx.abs() - 22.0).max(0.0);
            let by = (dy.abs() - 22.0).max(0.0);
            let in_badge = (bx * bx + by * by).sqrt() <= 8.5;

            let idx = ((y * w + x) * 4) as usize;
            if !in_badge {
                xor_bgra[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }

            let mut rgba = [18_u8, 20, 27, 255];
            if (21.0..=26.5).contains(&r) {
                rgba = [232, 114, 62, 255];
            } else if (12.0..21.0).contains(&r) {
                let ang = dy.atan2(dx);
                if ((ang * 3.0).cos()).abs() > 0.65 {
                    rgba = [34, 38, 49, 255];
                } else {
                    rgba = [56, 34, 28, 255];
                }
            } else if r <= 7.5 {
                rgba = if r <= 3.2 {
                    [16, 18, 24, 255]
                } else {
                    [66, 196, 176, 255]
                };
            }

            let wave_y =
                32.0 - 9.0 * ((fx - 32.0) * 0.35).sin() * (-((fx - 32.0).abs() * 0.06)).exp();
            if (src_y - wave_y).abs() < 1.35 && r < 24.0 {
                rgba = [245, 248, 255, 255];
            }

            // BGRA byte order
            xor_bgra[idx] = rgba[2];
            xor_bgra[idx + 1] = rgba[1];
            xor_bgra[idx + 2] = rgba[0];
            xor_bgra[idx + 3] = rgba[3];
        }
    }

    let bih_size = 40u32;
    let image_bytes = bih_size + xor_bgra.len() as u32 + and_mask.len() as u32;
    let mut ico = Vec::with_capacity(22 + image_bytes as usize);

    // ICONDIR (6 bytes)
    ico.extend_from_slice(&0u16.to_le_bytes()); // Reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // Type = 1 (Icon)
    ico.extend_from_slice(&1u16.to_le_bytes()); // Count = 1

    // ICONDIRENTRY (16 bytes)
    ico.push(w as u8);
    ico.push(h as u8);
    ico.push(0); // Color count
    ico.push(0); // Reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // Planes
    ico.extend_from_slice(&32u16.to_le_bytes()); // BitCount
    ico.extend_from_slice(&image_bytes.to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes()); // Offset

    // BITMAPINFOHEADER (40 bytes, biHeight = 2 * h for XOR + AND masks)
    ico.extend_from_slice(&bih_size.to_le_bytes());
    ico.extend_from_slice(&(w as i32).to_le_bytes());
    ico.extend_from_slice(&((h * 2) as i32).to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    ico.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    ico.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    ico.extend_from_slice(&(xor_bgra.len() as u32).to_le_bytes());
    ico.extend_from_slice(&0i32.to_le_bytes());
    ico.extend_from_slice(&0i32.to_le_bytes());
    ico.extend_from_slice(&0u32.to_le_bytes());
    ico.extend_from_slice(&0u32.to_le_bytes());

    ico.extend_from_slice(&xor_bgra);
    ico.extend_from_slice(&and_mask);

    fs::write(path, ico)
}

fn embed_windows_resources(ico_path: &Path, target_env: &str) {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let rc_path = out_dir.join("ferricdaw.rc");
    let escaped_ico = ico_path.to_string_lossy().replace('\\', "\\\\");

    let rc_content = format!(
        r#"
1 ICON "{ico}"

1 VERSIONINFO
FILEVERSION 1,0,0,0
PRODUCTVERSION 1,0,0,0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "FerricDAW Open Source Contributors"
            VALUE "FileDescription", "FerricDAW - Open-Source Rust Hybrid Digital Audio Workstation"
            VALUE "FileVersion", "1.0.0.0"
            VALUE "InternalName", "ferricdaw.exe"
            VALUE "LegalCopyright", "Copyright (c) 2026 FerricDAW Contributors (MIT OR Apache-2.0)"
            VALUE "OriginalFilename", "ferricdaw.exe"
            VALUE "ProductName", "FerricDAW"
            VALUE "ProductVersion", "1.0.0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#,
        ico = escaped_ico
    );

    if fs::write(&rc_path, rc_content).is_err() {
        return;
    }

    if target_env == "gnu" {
        let obj_path = out_dir.join("ferricdaw_res.o");
        if let Ok(status) = Command::new("windres")
            .arg(&rc_path)
            .arg("-O")
            .arg("coff")
            .arg("-o")
            .arg(&obj_path)
            .status()
        {
            if status.success() && obj_path.exists() {
                println!("cargo:rustc-link-arg-bins={}", obj_path.display());
            }
        }
    } else if target_env == "msvc" {
        let res_path = out_dir.join("ferricdaw.res");
        if let Ok(status) = Command::new("rc.exe")
            .arg("/fo")
            .arg(&res_path)
            .arg(&rc_path)
            .status()
        {
            if status.success() && res_path.exists() {
                println!("cargo:rustc-link-arg-bins={}", res_path.display());
            }
        }
    }
}
