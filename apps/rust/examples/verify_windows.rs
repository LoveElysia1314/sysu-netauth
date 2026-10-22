//! Build-time acceptance check for Windows release artifacts; never shipped.
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

struct Pe {
    bytes: Vec<u8>,
    optional: usize,
    sections: Vec<(u32, u32, usize, usize)>,
}
impl Pe {
    fn u16(bytes: &[u8], at: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(
            bytes
                .get(at..at.checked_add(2).context("offset overflow")?)
                .context("truncated PE")?
                .try_into()?,
        ))
    }
    fn u32(bytes: &[u8], at: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(
            bytes
                .get(at..at.checked_add(4).context("offset overflow")?)
                .context("truncated PE")?
                .try_into()?,
        ))
    }
    fn open(path: &Path, machine: u16) -> Result<Self> {
        let bytes = fs::read(path)?;
        ensure!(bytes.starts_with(b"MZ"), "not a Windows executable");
        let coff = Self::u32(&bytes, 0x3c)? as usize + 4;
        ensure!(
            bytes.get(coff - 4..coff) == Some(&b"PE\0\0"[..]),
            "invalid PE signature"
        );
        ensure!(
            Self::u16(&bytes, coff)? == machine,
            "incorrect PE machine: expected {machine:#06x}"
        );
        let count = Self::u16(&bytes, coff + 2)? as usize;
        ensure!(count <= 96, "invalid section count");
        let optional = coff + 20;
        ensure!(Self::u16(&bytes, optional)? == 0x20b, "expected PE32+");
        let table = optional + Self::u16(&bytes, coff + 16)? as usize;
        let mut sections = Vec::new();
        for i in 0..count {
            let at = table + i * 40;
            sections.push((
                Self::u32(&bytes, at + 12)?,
                Self::u32(&bytes, at + 8)?,
                Self::u32(&bytes, at + 20)? as usize,
                Self::u32(&bytes, at + 16)? as usize,
            ));
        }
        Ok(Self {
            bytes,
            optional,
            sections,
        })
    }
    fn offset(&self, rva: u32) -> Result<usize> {
        for &(start, size, raw, raw_size) in &self.sections {
            if let Some(delta) = rva.checked_sub(start) {
                if delta < size.max(raw_size as u32) {
                    ensure!((delta as usize) < raw_size, "RVA has no file data");
                    return raw.checked_add(delta as usize).context("offset overflow");
                }
            }
        }
        anyhow::bail!("RVA outside PE sections")
    }
    fn directory(&self, index: usize) -> Result<u32> {
        ensure!(
            Self::u32(&self.bytes, self.optional + 108)? as usize > index,
            "missing PE directory"
        );
        Self::u32(&self.bytes, self.optional + 112 + index * 8)
    }
    fn imports(&self) -> Result<Vec<String>> {
        let mut at = self.offset(self.directory(1)?)?;
        let mut imports = Vec::new();
        for _ in 0..256 {
            let name = Self::u32(&self.bytes, at + 12)?;
            if name == 0 {
                return Ok(imports);
            }
            let offset = self.offset(name)?;
            let tail = self.bytes.get(offset..).context("truncated import name")?;
            let len = tail
                .iter()
                .take(512)
                .position(|b| *b == 0)
                .context("invalid import name")?;
            imports.push(std::str::from_utf8(&tail[..len])?.to_ascii_lowercase());
            at += 20;
        }
        anyhow::bail!("unterminated import table")
    }
    fn resource_entry(&self, root: usize, directory: usize, id: u32) -> Result<u32> {
        let dir = root
            .checked_add(directory)
            .context("resource offset overflow")?;
        let count =
            Self::u16(&self.bytes, dir + 12)? as usize + Self::u16(&self.bytes, dir + 14)? as usize;
        for i in 0..count {
            let at = dir + 16 + i * 8;
            if Self::u32(&self.bytes, at)? == id {
                return Self::u32(&self.bytes, at + 4);
            }
        }
        anyhow::bail!("missing resource type/id {id}")
    }
    fn resource(&self, kind: u32, id: u32) -> Result<&[u8]> {
        let root = self.offset(self.directory(2)?)?;
        let types = self.resource_entry(root, 0, kind)?;
        ensure!(types & 0x80000000 != 0, "invalid resource type directory");
        let languages = self.resource_entry(root, (types & 0x7fffffff) as usize, id)?;
        ensure!(
            languages & 0x80000000 != 0,
            "invalid resource language directory"
        );
        let languages = root + (languages & 0x7fffffff) as usize;
        ensure!(
            Self::u16(&self.bytes, languages + 12)? as usize
                + Self::u16(&self.bytes, languages + 14)? as usize
                > 0,
            "resource has no language"
        );
        let entry = Self::u32(&self.bytes, languages + 20)?;
        ensure!(entry & 0x80000000 == 0, "invalid resource data entry");
        let at = root + entry as usize;
        let data = self.offset(Self::u32(&self.bytes, at)?)?;
        let size = Self::u32(&self.bytes, at + 4)? as usize;
        self.bytes
            .get(data..data.checked_add(size).context("resource size overflow")?)
            .context("truncated resource data")
    }
}

fn verify(path: &Path, gui: bool, machine: u16) -> Result<()> {
    let pe = Pe::open(path, machine)?;
    let subsystem = Pe::u16(&pe.bytes, pe.optional + 68)?;
    ensure!(
        subsystem == if gui { 2 } else { 3 },
        "incorrect executable subsystem"
    );
    let imports = pe.imports()?;
    for dll in &imports {
        ensure!(
            ![
                "vcruntime",
                "msvcp",
                "api-ms-win-crt-",
                "libgcc",
                "libstdc++",
                "libwinpthread"
            ]
            .iter()
            .any(|prefix| dll.starts_with(prefix))
                && dll != "ucrtbase.dll",
            "dynamic compiler/CRT dependency: {dll}"
        );
        ensure!(
            dll != "wpcap.dll" && dll != "packet.dll",
            "Npcap must be loaded only when needed"
        );
    }
    if gui {
        let manifest = std::str::from_utf8(pe.resource(24, 1)?)?;
        ensure!(
            manifest.contains("level=\"requireAdministrator\""),
            "missing GUI elevation manifest"
        );
        ensure!(
            !manifest.contains("processorArchitecture=\"amd64\"")
                && !manifest.contains("processorArchitecture=\"x86\""),
            "GUI manifest must support both x64 and ARM64"
        );
        for id in 1..=6 {
            ensure!(
                !pe.resource(14, id)?.is_empty(),
                "missing embedded icon {id}"
            );
        }
    }
    println!(
        "{}: {} bytes; no external compiler runtime; {}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        pe.bytes.len(),
        if gui {
            "GUI, elevation manifest and six icons verified"
        } else {
            "console"
        }
    );
    println!("  OS imports: {}", imports.join(", "));
    Ok(())
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let dir = args
        .next()
        .context("usage: verify_windows RELEASE_DIRECTORY [x64|arm64]")?;
    let arch = args.next().unwrap_or_else(|| "x64".into());
    let machine = match arch.to_str() {
        Some("x64") => 0x8664,
        Some("arm64") => 0xaa64,
        _ => anyhow::bail!("unsupported Windows architecture: {arch:?}"),
    };
    ensure!(args.next().is_none(), "unexpected verifier argument");
    let dir = Path::new(&dir);
    for name in ["sysu_netauth", "sysu_netauth_service", "sysu_netauth_gui"] {
        verify(
            &dir.join(format!("{name}.exe")),
            name.ends_with("gui"),
            machine,
        )
        .with_context(|| format!("verify {name}"))?;
    }
    Ok(())
}
