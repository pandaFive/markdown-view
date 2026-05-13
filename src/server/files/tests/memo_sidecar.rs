#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::server::files::memo::{sidecar_parent_for_target_path, sidecar_parent_or_base};
use crate::server::files::memo_sidecar::SidecarMemoName;

fn assert_plain_sidecar_filename(name: &str) {
    let path = Path::new(name);
    assert!(path.parent().is_none() || path.parent() == Some(Path::new("")));
    assert_eq!(
        path.file_name().and_then(|file_name| file_name.to_str()),
        Some(name)
    );
}

#[test]
fn test_sidecar_name_超長名は255バイト以内に短縮される() {
    let file_name = format!("{}.md", "a".repeat(251));
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.starts_with("."));
    assert!(name.ends_with(".memo.md"));
    assert!(name.len() <= 255, "sidecar名が長すぎる: {}", name.len());
}

#[test]
fn test_sidecar_name_ファイル名なしfallbackは従来名を保つ() {
    let sidecar = SidecarMemoName::fallback();
    assert_plain_sidecar_filename(sidecar.as_str());
    assert_eq!(sidecar.as_str(), ".memo.md");
}

#[test]
fn test_sidecar_name_空ファイル名はfallbackを返す() {
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(""));
    assert_plain_sidecar_filename(sidecar.as_str());
    assert_eq!(sidecar.as_str(), ".memo.md");
}

#[test]
fn test_sidecar_name_同一prefixの超長名はhashで衝突しない() {
    let common_prefix = "a".repeat(260);
    let first_name = format!("{common_prefix}-first.md");
    let second_name = format!("{common_prefix}-second.md");
    let first = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&first_name));
    let second = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&second_name));
    assert_plain_sidecar_filename(first.as_str());
    assert_plain_sidecar_filename(second.as_str());
    assert_ne!(first.as_str(), second.as_str());
    assert!(first.as_str().len() <= 255);
    assert!(second.as_str().len() <= 255);
}

#[test]
fn test_sidecar_name_特殊文字はパス区切りとして扱われない() {
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new("../secret\\..\\memo.md"));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.ends_with(".memo.md"));
    assert!(!name.contains('/'), "slashが残ってはいけない: {name}");
    assert!(!name.contains('\\'), "backslashが残ってはいけない: {name}");
    assert!(
        name.contains(".."),
        "通常文字としてのdotは保持してよい: {name}"
    );
}

#[test]
fn test_sidecar_name_正規化された短い名前はhashで衝突しない() {
    let plain = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a_b.md"));
    let normalized = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    assert_plain_sidecar_filename(plain.as_str());
    assert_plain_sidecar_filename(normalized.as_str());
    assert_eq!(plain.as_str(), ".a_b.md.memo.md");
    assert!(normalized.as_str().starts_with(".a_b.md."));
    assert!(normalized.as_str().ends_with(".memo.md"));
    assert_ne!(plain.as_str(), normalized.as_str());
}

#[cfg(unix)]
#[test]
fn test_sidecar_name_旧形式compat名は正規化前の名前を返す() {
    let compat = SidecarMemoName::compat_from_file_name(std::ffi::OsStr::new("a\\b.md"))
        .expect("backslash name should have compat sidecar");
    assert_plain_sidecar_filename(compat.as_str());
    assert_eq!(compat.as_str(), ".a\\b.md.memo.md");

    assert!(SidecarMemoName::compat_from_file_name(std::ffi::OsStr::new("a_b.md")).is_none());
    assert!(SidecarMemoName::compat_from_file_name(std::ffi::OsStr::new("a/b\\c.md")).is_none());
}

#[cfg(unix)]
#[test]
fn test_sidecar_name_旧形式compat名の超長名は255バイト以下に短縮される() {
    let file_name = format!("{}\\{}.md", "a".repeat(180), "b".repeat(120));
    assert_eq!(file_name.len(), 304);

    let compat = SidecarMemoName::compat_from_file_name(std::ffi::OsStr::new(&file_name))
        .expect("backslash name should have compat sidecar");
    let name = compat.as_str();
    assert_plain_sidecar_filename(name);
    assert!(
        name.len() <= 255,
        "compat sidecar名が長すぎる: {}",
        name.len()
    );
    assert!(name.ends_with(".memo.md"), ".memo.md 終端: {name}");

    let parts: Vec<&str> = name.split('.').collect();
    assert_eq!(parts.len(), 5, "compat hash 経路は 4 dot 区切り: {name}");
    let hash = parts[2];
    assert_eq!(hash.len(), 16, "hash suffix は 16 hex chars: {hash}");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash suffix は hex のみ: {hash}"
    );
}

#[test]
fn test_sidecar_name_utf8境界で切り詰める() {
    let file_name = format!("{}終端.md", "あ".repeat(120));
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.ends_with(".memo.md"));
    assert!(name.len() <= 255);
    assert!(name.is_char_boundary(name.len()));
}

#[cfg(unix)]
#[test]
fn test_sidecar_name_非utf8名はhashで衝突しない() {
    let first = SidecarMemoName::from_file_name(std::ffi::OsStr::from_bytes(b"guide-\xff.md"));
    let second = SidecarMemoName::from_file_name(std::ffi::OsStr::from_bytes(b"guide-\xfe.md"));
    assert_plain_sidecar_filename(first.as_str());
    assert_plain_sidecar_filename(second.as_str());
    assert!(first.as_str().starts_with("._bin."));
    assert!(second.as_str().starts_with("._bin."));
    assert!(first.as_str().ends_with(".memo.md"));
    assert!(second.as_str().ends_with(".memo.md"));
    assert_ne!(first.as_str(), second.as_str());
}

#[test]
fn test_sidecar_name_255バイト境界はそのまま使う() {
    // .{name}.memo.md = 1 + name + 8 = 255 byte ぴったりに収まる input
    // → name = 246 → "a"*243 + ".md"
    let file_name = format!("{}.md", "a".repeat(243));
    assert_eq!(file_name.len(), 246);
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert_eq!(name.len(), 255, "境界ちょうどはそのまま使う");
    assert_eq!(name, format!(".{}.memo.md", file_name));
    // hash 経路に入っていないことの確認: 元 filename がそのまま含まれる
    assert!(name.contains(&"a".repeat(243)));
}

#[test]
fn test_sidecar_name_256バイト境界はhash経路に入る() {
    // 1 byte 超過で hash truncation 経路
    // .{name}.memo.md = 1 + 247 + 8 = 256 → hash 経路
    let file_name = format!("{}.md", "a".repeat(244));
    assert_eq!(file_name.len(), 247);
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(
        name.len() <= 255,
        "boundary 直上で 255 を超えてはいけない: {}",
        name.len()
    );
    // hash 経路の形式確認: . + prefix + . + 16hex + .memo.md
    // split('.') で ["", prefix, hash, "memo", "md"] の 5 要素
    let parts: Vec<&str> = name.split('.').collect();
    assert_eq!(parts.len(), 5, "hash 経路は 4 dot 区切り: {name}");
    let hash = parts[2];
    assert_eq!(hash.len(), 16, "hash suffix は 16 hex chars: {hash}");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash suffix は hex のみ: {hash}"
    );
}

#[test]
fn test_sidecar_name_utf8マルチバイト境界の直前で切断する() {
    // prefix budget = 255 - (1+1+16+8) = 229 bytes
    // budget の境目に 3-byte UTF-8 char (`あ`) を置き、char 境界で切断されることを確認
    // input: "あ"*90 + "tail.md" → 270 + 7 = 277 bytes (hash 経路に確実に入る)
    let file_name = format!("{}tail.md", "あ".repeat(90));
    assert_eq!(file_name.len(), 277);
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.len() <= 255);
    let parts: Vec<&str> = name.split('.').collect();
    assert_eq!(parts.len(), 5, "hash 経路は 4 dot 区切り: {name}");
    let prefix = parts[1];
    assert_eq!(
        prefix.len(),
        228,
        "prefix は 229 byte budget 直前の char 境界で切断される: {prefix}"
    );
    assert_eq!(
        prefix,
        "あ".repeat(76),
        "prefix は 3-byte UTF-8 char 76 文字分で切断される"
    );
    let hash = parts[2];
    assert_eq!(hash.len(), 16, "hash suffix は 16 hex chars: {hash}");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash suffix は hex のみ: {hash}"
    );
    assert_eq!(parts[3], "memo", "hash 経路 suffix は .memo.md: {name}");
    assert_eq!(parts[4], "md", "hash 経路 suffix は .memo.md: {name}");
    // 切断点が UTF-8 char 境界にあること（&str[..end] は char boundary を要求するため、
    //   ここまで到達できている時点で UTF-8 として valid。明示的にも検証）
    assert!(name.is_char_boundary(name.len()));
    assert!(
        std::str::from_utf8(name.as_bytes()).is_ok(),
        "valid UTF-8: {name}"
    );
}

#[test]
fn test_sidecar_name_パス区切り含む超長名でも255以下_衝突しない() {
    // 区切り種別と位置が異なる 2 つの 300+ byte input で衝突しないこと
    let file_name1 = format!("{}/{}.md", "a".repeat(200), "b".repeat(100));
    let file_name2 = format!("{}\\{}.md", "a".repeat(150), "b".repeat(150));
    assert_eq!(file_name1.len(), 304);
    assert_eq!(file_name2.len(), 304);
    let first = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name1));
    let second = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name2));
    let n1 = first.as_str();
    let n2 = second.as_str();
    assert_plain_sidecar_filename(n1);
    assert_plain_sidecar_filename(n2);
    assert!(n1.len() <= 255);
    assert!(n2.len() <= 255);
    assert!(
        !n1.contains('/') && !n1.contains('\\'),
        "パス区切りが残らない: {n1}"
    );
    assert!(
        !n2.contains('/') && !n2.contains('\\'),
        "パス区切りが残らない: {n2}"
    );
    assert_ne!(n1, n2, "区切り種別 / 位置が異なる場合は衝突しない");
}

#[test]
fn test_sidecar_name_任意入力で常に255バイト以下_不変条件_utf8() {
    // UTF-8 入力の網羅的境界ケース: 出力が常に MAX_FILENAME_BYTES (= 255) 以下
    let cases: Vec<String> = vec![
        String::new(),
        "a".to_string(),
        "a".repeat(254),
        "a".repeat(255),
        "a".repeat(256),
        "a".repeat(1000),
        "あ".repeat(100),
        "../../etc/passwd".to_string(),
        "a/b\\c".to_string(),
        "./relative/path/with/many/segments.md".to_string(),
    ];
    for input in cases {
        let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&input));
        let name = sidecar.as_str();
        assert!(
            name.len() <= 255,
            "input {} bytes → output {} bytes (255 超過): {name}",
            input.len(),
            name.len()
        );
        assert!(name.ends_with(".memo.md"), ".memo.md 終端: {name}");
        assert!(name.starts_with('.'), ". 開始: {name}");
        assert!(!name.contains('/'), "/ 含まない: {name}");
        assert!(!name.contains('\\'), "\\ 含まない: {name}");
    }
}

#[cfg(unix)]
#[test]
fn test_sidecar_name_任意入力で常に255バイト以下_不変条件_非utf8() {
    use std::os::unix::ffi::OsStrExt;

    let cases: Vec<Vec<u8>> = vec![
        b"\xff\xfe\xfd".to_vec(),
        vec![0xff; 100],
        vec![0xff; 255],
        vec![0xff; 1000],
        b"a/\xff.md".to_vec(),
        b"a\\\xff.md".to_vec(),
    ];
    for raw in cases {
        let os_str = std::ffi::OsStr::from_bytes(&raw);
        let sidecar = SidecarMemoName::from_file_name(os_str);
        let name = sidecar.as_str();
        assert_plain_sidecar_filename(name);
        assert!(
            name.len() <= 255,
            "非UTF-8 input {} bytes → output {} bytes",
            raw.len(),
            name.len()
        );
        assert!(!name.contains('/'), "/ 含まない: {name}");
        assert!(!name.contains('\\'), "\\ 含まない: {name}");
        let parts: Vec<&str> = name.split('.').collect();
        assert_eq!(
            parts,
            vec!["", "_bin", parts[2], "memo", "md"],
            "非UTF-8 fallback 形式: {name}"
        );
        let hash = parts[2];
        assert_eq!(hash.len(), 16, "hash suffix は 16 hex chars: {hash}");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash suffix は hex のみ: {hash}"
        );
        assert!(name.ends_with(".memo.md"));
    }
}

#[test]
fn test_sidecar_parent_相対パスはbase_dirへfallbackする() {
    let base_dir = Path::new("/tmp/markdown-view-base");

    let parent = sidecar_parent_or_base(Path::new("memo.md"), base_dir);

    assert_eq!(parent, base_dir);
}

#[test]
fn test_sidecar_parent_絶対パスは親ディレクトリを使う() {
    let base_dir = Path::new("/tmp/markdown-view-base");

    let parent = sidecar_parent_for_target_path(Path::new("/tmp/docs/memo.md"), base_dir);

    assert_eq!(parent, Path::new("/tmp/docs"));
}
