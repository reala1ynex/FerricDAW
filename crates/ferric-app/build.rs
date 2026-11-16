use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let ico_path = out_dir.join("ferricdaw.ico");
    let rgba = lattice_icon();
    fs::write(out_dir.join("ferricdaw.rgba"), &rgba).expect("write FerricDAW app icon");
    write_ferric_ico(&ico_path, &rgba).expect("write FerricDAW Windows icon");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" {
        embed_windows_resources(&ico_path, &target_env);
    }
}

/// Flat, antialiased lattice for small desktop icon sizes. Front is the
/// orange audio layer, back is the cyan control layer, centre is the engine.
fn lattice_icon() -> Vec<u8> {
    const SIZE: usize = 64;
    const ORANGE: [u8; 3] = [255, 90, 31];
    const CYAN: [u8; 3] = [0, 229, 255];
    const VOID: [u8; 3] = [7, 8, 10];
    let back = [(22.0, 12.0), (56.0, 12.0), (56.0, 40.0), (22.0, 40.0)];
    let front = [(8.0, 25.0), (51.0, 25.0), (51.0, 54.0), (8.0, 54.0)];
    let wave = [(22.0, 40.0), (26.0, 40.0), (29.0, 34.5), (32.0, 44.5),
        (35.0, 38.0), (38.0, 40.0)];
    fn distance(p: (f32, f32), q: (f32, f32)) -> f32 {
        (p.0 - q.0).hypot(p.1 - q.1)
    }
    fn line(p: (f32, f32), a: (f32, f32), b: (f32, f32), width: f32) -> bool {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (((p.0-a.0)*dx + (p.1-a.1)*dy) / (dx*dx + dy*dy)).clamp(0.0, 1.0);
        distance(p, (a.0 + dx*t, a.1 + dy*t)) <= width * 0.5
    }
    let sample = |p| {
        let mut c = VOID;
        for i in 0..4 {
            if line(p, back[i], back[(i+1)%4], 2.1) || line(p, front[i], back[i], 1.7) {
                c = CYAN;
            }
        }
        for &v in &back {
            let d = distance(p, v);
            if d <= 3.4 { c = if d > 1.55 { CYAN } else { VOID }; }
        }
        for i in 0..4 {
            if line(p, front[i], front[(i+1)%4], 3.0) {
                c = ORANGE;
            } else if line(p, front[i], (29.5, 39.5), 1.0) && c == VOID {
                c = [125, 43, 18];
            }
        }
        for &v in &front {
            let d = distance(p, v);
            if d <= 4.8 { c = if d > 2.5 { ORANGE } else { VOID }; }
        }
        if distance(p, (29.5, 39.5)) <= 8.7 {
            c = if wave.windows(2).any(|q| line(p, q[0], q[1], 2.1)) { VOID } else { ORANGE };
        }
        c
    };
    let mut rgba = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut sum = [0u16; 3];
            for sy in [0.25, 0.75] {
                for sx in [0.25, 0.75] {
                    for (total, channel) in sum.iter_mut().zip(sample((x as f32+sx, y as f32+sy))) {
                        *total += u16::from(channel);
                    }
                }
            }
            rgba.extend(sum.map(|channel| (channel/4) as u8));
            rgba.push(255);
        }
    }
    rgba
}

fn write_ferric_ico(path: &Path, rgba: &[u8]) -> std::io::Result<()> {
    let w = 64u32;
    let h = 64u32;
    let mut xor_bgra = Vec::with_capacity(rgba.len());
    let and_mask = vec![0u8; (h * 8) as usize];
    for row in rgba.chunks_exact(w as usize * 4).rev() {
        for px in row.chunks_exact(4) {
            xor_bgra.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
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

    let version = env!("CARGO_PKG_VERSION");
    let resource_version = format!(
        "{},{},{},0",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    let rc_content = format!(
        r#"
1 ICON "{ico}"

1 VERSIONINFO
FILEVERSION {resource_version}
PRODUCTVERSION {resource_version}
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
            VALUE "FileVersion", "{version}.0"
            VALUE "InternalName", "ferricdaw.exe"
            VALUE "LegalCopyright", "Copyright (c) 2026 FerricDAW Contributors (MIT OR Apache-2.0)"
            VALUE "OriginalFilename", "ferricdaw.exe"
            VALUE "ProductName", "FerricDAW"
            VALUE "ProductVersion", "{version}"
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
