#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::{fs, os::unix::fs::symlink};

use axum::http::StatusCode;
use axum::response::IntoResponse;
use tokio::sync::broadcast;

use super::catalog::{canonicalize_dir_for_cycle, MAX_DIR_DEPTH, MAX_FILE_LIST};
use super::content::{read_bytes_with_limit, ReadMarkdownError};
use super::memo::{sidecar_parent_for_target_path, sidecar_parent_or_base};
use super::memo_sidecar::SidecarMemoName;
use super::resolve::revalidate_single_file_target;
use super::test_support::{make_test_app_state, MockMemoFs, Op, TempWorkspace};
use super::*;
use crate::server::{AppMode, AppState, BroadcastMessage};

fn assert_plain_sidecar_filename(name: &str) {
    let path = Path::new(name);
    assert!(path.parent().is_none() || path.parent() == Some(Path::new("")));
    assert_eq!(
        path.file_name().and_then(|file_name| file_name.to_str()),
        Some(name)
    );
}

#[test]
fn test_close_code_ioエラーは1011を返す() {
    let err = ReadMarkdownError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, ""));
    assert_eq!(err.close_code(), 1011);
}

#[test]
fn test_close_code_too_largeは1009を返す() {
    let err = ReadMarkdownError::TooLarge;
    assert_eq!(err.close_code(), 1009);
}

#[test]
fn test_close_code_not_utf8は1003を返す() {
    let err = ReadMarkdownError::NotUtf8;
    assert_eq!(err.close_code(), 1003);
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

#[test]
fn test_resolve_file_正常なパス() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "README.md");
    assert!(result.is_ok());
    assert!(result.unwrap().ends_with("README.md"));
}

#[test]
fn test_resolve_file_サブディレクトリのパス() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "docs/api.md");
    assert!(result.is_ok());
}

#[test]
fn test_resolve_file_トラバーサル拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "../../../etc/passwd");
    assert!(matches!(
        result,
        Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
    ));
}

#[test]
fn test_resolve_file_バックスラッシュ型トラバーサルを拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "..\\..\\..\\etc\\passwd");
    assert!(matches!(
        result,
        Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
    ));
}

#[test]
fn test_resolve_file_urlエンコード型トラバーサルを拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "docs/%2e%2e/%2e%2e/etc/passwd.md");
    assert!(matches!(
        result,
        Err(ResolveFileError::NotFound) | Err(ResolveFileError::Traversal)
    ));
}

#[test]
fn test_resolve_file_絶対パス拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "/etc/passwd");
    assert_eq!(result, Err(ResolveFileError::InvalidPath));
}

#[test]
fn test_resolve_file_存在しないファイル() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "nonexistent.md");
    assert_eq!(result, Err(ResolveFileError::NotFound));
}

#[test]
fn test_resolve_file_非md拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "notes.txt");
    assert_eq!(result, Err(ResolveFileError::NotMarkdown));
}

#[test]
fn test_resolve_file_隠しファイル拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), ".hidden/secret.md");
    assert_eq!(result, Err(ResolveFileError::Hidden));
}

#[test]
fn test_resolve_file_隠しドットファイル拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), ".dotfile.md");
    assert_eq!(result, Err(ResolveFileError::Hidden));
}

#[test]
fn test_resolve_file_nulバイト拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "README\0.md");
    assert_eq!(result, Err(ResolveFileError::InvalidPath));
}

#[test]
fn test_resolve_file_ディレクトリパス拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "docs");
    assert_eq!(result, Err(ResolveFileError::NotFound));
}

#[test]
fn test_resolve_file_空パス拒否() {
    let dir = create_test_dir();
    let result = resolve_file(dir.path(), "");
    assert_eq!(result, Err(ResolveFileError::EmptyPath));
}

#[cfg(unix)]
#[test]
fn test_resolve_file_シンボリックリンクによるトラバーサル拒否() {
    let dir = create_test_dir();
    let outside_dir = tempfile::tempdir().unwrap();
    std::fs::write(outside_dir.path().join("secret.md"), "# Secret").unwrap();

    std::os::unix::fs::symlink(
        outside_dir.path().join("secret.md"),
        dir.path().join("link.md"),
    )
    .unwrap();

    let result = resolve_file(dir.path(), "link.md");
    assert_eq!(result, Err(ResolveFileError::Traversal));
}

#[test]
fn test_list_markdown_files_基本動作() {
    let dir = create_test_dir();
    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.contains(&"README.md".to_string()));
    assert!(files.contains(&"guide.md".to_string()));
    assert!(files.contains(&"docs/api.md".to_string()));
}

#[test]
fn test_list_markdown_files_非md除外() {
    let dir = create_test_dir();
    let files = list_markdown_files(dir.path()).unwrap();
    assert!(!files.iter().any(|f| f.ends_with(".txt")));
}

#[test]
fn test_list_markdown_files_隠しファイル除外() {
    let dir = create_test_dir();
    let files = list_markdown_files(dir.path()).unwrap();
    assert!(!files.iter().any(|f| f.contains(".hidden")));
    assert!(!files.iter().any(|f| f.starts_with('.')));
}

#[test]
fn test_list_markdown_files_空ディレクトリ() {
    let dir = tempfile::tempdir().unwrap();
    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.is_empty());
}

#[test]
fn test_list_markdown_files_ソート済み() {
    let dir = create_test_dir();
    let files = list_markdown_files(dir.path()).unwrap();
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted);
}

#[test]
fn test_list_markdown_files_最大1000件で打ち切る() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..(MAX_FILE_LIST + 200) {
        let path = dir.path().join(format!("doc-{i:04}.md"));
        std::fs::write(path, "# x").unwrap();
    }

    let files = list_markdown_files(dir.path()).unwrap();
    assert_eq!(files.len(), MAX_FILE_LIST);
}

#[test]
fn test_list_markdown_files_ベースディレクトリ正規化失敗はエラーを返す() {
    let missing = PathBuf::from("/path/that/does/not/exist");
    let result = list_markdown_files(&missing);
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn test_list_markdown_files_シンボリックリンクサイクルでハングしない() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# README").unwrap();
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/doc.md"), "# Doc").unwrap();

    std::os::unix::fs::symlink(dir.path(), dir.path().join("sub/loop")).unwrap();

    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.contains(&"README.md".to_string()));
    assert!(files.contains(&"sub/doc.md".to_string()));
    assert!(
        !files.iter().any(|f| f.contains("loop/")),
        "サイクル経由のエントリが含まれてはいけない: {:?}",
        files
    );
}

#[cfg(unix)]
#[test]
fn test_list_markdown_files_自己参照シンボリックリンクでハングしない() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# README").unwrap();

    std::os::unix::fs::symlink(".", dir.path().join("loop")).unwrap();

    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.contains(&"README.md".to_string()));
    assert!(
        !files.iter().any(|f| f.contains("loop/")),
        "サイクル経由のエントリが含まれてはいけない: {:?}",
        files
    );
}

#[test]
fn test_list_markdown_files_深度上限を超えるパスは除外される() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("root.md"), "# root").unwrap();

    let mut current = dir.path().to_path_buf();
    for i in 0..=MAX_DIR_DEPTH {
        current = current.join(format!("d{}", i));
        std::fs::create_dir_all(&current).unwrap();
    }
    std::fs::write(current.join("deep.md"), "# deep").unwrap();

    let files = list_markdown_files(dir.path()).unwrap();
    assert!(files.contains(&"root.md".to_string()));
    assert!(!files.iter().any(|f| f.ends_with("deep.md")));
}

#[test]
fn test_list_markdown_files_recursive_通常ディレクトリcanonicalize失敗時はスキップ扱い() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-dir");
    assert!(canonicalize_dir_for_cycle(&missing, "通常ディレクトリ", dir.path()).is_none());
}

#[tokio::test]
async fn test_read_bytes_with_limit_takeによる第2段階チェックで超過を検出する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("large.md");
    tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
        .await
        .unwrap();

    let file = tokio::fs::File::open(&file_path).await.unwrap();
    let result = read_bytes_with_limit(file).await;
    assert!(matches!(result, Err(ReadMarkdownError::TooLarge)));
}

#[tokio::test]
async fn test_read_markdown_error_into_response_too_largeのjson形式() {
    let response = ReadMarkdownError::TooLarge.into_response();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "error": "ファイルサイズが上限（10MB）を超えています"
        })
    );
}

#[tokio::test]
async fn test_read_markdown_error_into_response_ioのjson形式() {
    let io_error = std::io::Error::other("disk failure");
    let response = ReadMarkdownError::Io(io_error).into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "error": "ファイルの読み込みに失敗しました"
        })
    );
}

#[tokio::test]
async fn test_read_markdown_error_into_response_not_utf8のjson形式() {
    let response = ReadMarkdownError::NotUtf8.into_response();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "error": "このファイルはUTF-8テキストではありません"
        })
    );
}

#[test]
fn test_resolve_route_target_page_ディレクトリモードでrelative_pathとfile_listを返す() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let target =
        resolve_route_target(&state, RouteTargetRequest::page(Some("docs/api.md"))).unwrap();

    assert_eq!(target.relative_path(), Some("docs/api.md"));
    assert!(target.file_list().is_some());
    assert!(target.file_path().ends_with("docs/api.md"));
}

#[test]
fn test_resolve_route_target_api_contentはfile_listを含まない() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let target =
        resolve_route_target(&state, RouteTargetRequest::api_content(Some("docs/api.md"))).unwrap();

    assert_eq!(target.relative_path(), Some("docs/api.md"));
    assert!(target.file_list().is_none());
}

#[test]
fn test_resolve_route_target_page_queryなしではreadmeを優先する() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("z-last.md"), "# z").unwrap();
    std::fs::write(dir.path().join("README.md"), "# readme").unwrap();
    let state = create_directory_state(dir.path());

    let target = resolve_route_target(&state, RouteTargetRequest::page(None)).unwrap();

    assert_eq!(target.relative_path(), Some("README.md"));
    assert!(target.file_path().ends_with("README.md"));
    assert_eq!(
        target.file_list().unwrap(),
        &["README.md".to_string(), "z-last.md".to_string()]
    );
}

#[test]
fn test_resolve_route_target_page_queryなしではreadme不在時に先頭ファイルを選ぶ() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("z-last.md"), "# z").unwrap();
    std::fs::write(dir.path().join("a-first.md"), "# a").unwrap();
    let state = create_directory_state(dir.path());

    let target = resolve_route_target(&state, RouteTargetRequest::page(None)).unwrap();

    assert_eq!(target.relative_path(), Some("a-first.md"));
    assert!(target.file_path().ends_with("a-first.md"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_旧メモルートがシンボリックリンクなら空メモとして扱う() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("README.md");
    fs::write(&file_path, "# README").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(outside_dir.path().join("memos")).unwrap();
    fs::write(outside_dir.path().join("memos/README.md"), "legacy memo").unwrap();
    symlink(outside_dir.path(), dir.path().join(".markdown-view")).unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("unsafe legacy should be ignored when no sidecar exists");

    assert_eq!(memo.raw(), "");
    assert_eq!(memo.html().as_str(), "");
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_新sidecarがシンボリックリンクなら拒否する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("README.md");
    fs::write(&file_path, "# README").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::write(outside_dir.path().join("memo.md"), "outside").unwrap();
    symlink(
        outside_dir.path().join("memo.md"),
        dir.path().join(".README.md.memo.md"),
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let result = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None)).await;

    let (status, body) = result.expect_err("unsafe primary sidecar should be rejected");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(
        json["error"],
        "メモ保存先にシンボリックリンクが含まれているため操作できません"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_新メモファイルがシンボリックリンクなら拒否する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::write(outside_dir.path().join("memo.md"), "outside").unwrap();
    symlink(
        outside_dir.path().join("memo.md"),
        dir.path().join(".README.md.memo.md"),
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("symlinked memo leaf should be rejected");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(
        json["error"],
        "メモ保存先にシンボリックリンクが含まれているため操作できません"
    );
}

#[tokio::test]
async fn test_save_route_memo_単一ファイルモードで同階層sidecarへ保存する() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("memo should save");

    assert_eq!(memo.raw(), "memo");
    assert!(file_path
        .parent()
        .unwrap()
        .join(".test.md.memo.md")
        .exists());
}

#[tokio::test]
async fn test_load_route_memo_旧パスのみ存在する場合はそのまま読み込む() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("legacy memo should load");

    assert_eq!(memo.raw(), "legacy memo");
    assert!(!dir.path().join(".README.md.memo.md").exists());
    assert!(dir.path().join(".markdown-view/memos/README.md").exists());
}

#[tokio::test]
async fn test_load_route_memo_新旧両方ある場合は新sidecarを優先する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::write(dir.path().join(".README.md.memo.md"), "new memo").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("new memo should win");

    assert_eq!(memo.raw(), "new memo");
    assert!(dir.path().join(".markdown-view/memos/README.md").exists());
}

#[tokio::test]
async fn test_save_route_memo_旧パスのみ存在する場合は新sidecarへ移行して保存する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    fs::write(
        dir.path().join(".markdown-view/memos/README.md"),
        "legacy memo",
    )
    .unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "updated memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("legacy memo should migrate on save");

    assert_eq!(memo.raw(), "updated memo");
    assert!(dir.path().join(".README.md.memo.md").exists());
    assert!(!dir.path().join(".markdown-view/memos/README.md").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_旧symlinkが残っていてもsidecar保存を継続できる() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    symlink(outside_dir.path(), dir.path().join(".markdown-view")).unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("unsafe legacy should not block sidecar save");

    assert_eq!(memo.raw(), "memo");
    assert!(dir.path().join(".README.md.memo.md").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_空白保存はunsafeなlegacyがあってもsidecar削除を優先する() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "# README").unwrap();
    fs::write(dir.path().join(".README.md.memo.md"), "memo").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(outside_dir.path().join("memos")).unwrap();
    fs::write(outside_dir.path().join("memos/README.md"), "legacy memo").unwrap();
    symlink(outside_dir.path(), dir.path().join(".markdown-view")).unwrap();

    let state = create_directory_state(dir.path());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "   \n".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("unsafe legacy should not block sidecar delete");

    assert_eq!(memo.raw(), "");
    assert!(!dir.path().join(".README.md.memo.md").exists());
    assert!(dir.path().join(".markdown-view").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_空白保存_safe_legacy削除失敗は500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    workspace
        .write_file(Path::new("README.md"), "# README")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".README.md.memo.md");
    workspace
        .write_file(Path::new(".README.md.memo.md"), "memo")
        .expect("sidecar memo should be written");
    let legacy_path = workspace.path().join(".markdown-view/memos/README.md");
    workspace
        .write_file(Path::new(".markdown-view/memos/README.md"), "legacy memo")
        .expect("legacy memo should be written");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        &legacy_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_directory(workspace.path()).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "   \n".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("safe legacy cleanup failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
    assert!(!sidecar_path.exists());
    assert!(legacy_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_空白保存_safe_compat削除失敗は500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("a\\b.md"), "# separator shaped")
        .expect("target markdown should be written");
    workspace
        .write_file(Path::new(".a_b.md.memo.md"), "memo")
        .expect("sidecar memo should be written");
    let compat_sidecar_path = workspace
        .write_file(Path::new(".a\\b.md.memo.md"), "compat memo")
        .expect("compat sidecar memo should be written");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        &compat_sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "   \n".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("safe compat cleanup failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
    assert!(compat_sidecar_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_保存成功後のlegacy削除失敗は成功扱いにする() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    workspace
        .write_file(Path::new("README.md"), "# README")
        .expect("target markdown should be written");
    let legacy_path = workspace.path().join(".markdown-view/memos/README.md");
    workspace
        .write_file(Path::new(".markdown-view/memos/README.md"), "legacy memo")
        .expect("legacy memo should be written");
    let sidecar_path = workspace.path().join(".README.md.memo.md");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        &legacy_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_directory(workspace.path()).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let saved = save_route_memo(
        &state,
        &target,
        "updated again".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("legacy cleanup failure should be non-fatal");

    assert_eq!(saved.raw(), "updated again");
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), "updated again");
    assert!(legacy_path.exists());
}

#[tokio::test]
async fn test_save_route_memo_拡張子の大文字小文字が異なるファイルでもsidecarが衝突しない() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("guide.md"), "# lower").unwrap();
    fs::write(dir.path().join("guide.MD"), "# upper").unwrap();
    let state = create_directory_state(dir.path());

    let lower_target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(Some("guide.md"))).unwrap();
    let upper_target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(Some("guide.MD"))).unwrap();

    let lower = save_route_memo(
        &state,
        &lower_target,
        "lower memo".to_string(),
        RouteTargetRequest::api_memo(Some("guide.md")),
    )
    .await
    .expect("lower memo should save");
    let upper = save_route_memo(
        &state,
        &upper_target,
        "upper memo".to_string(),
        RouteTargetRequest::api_memo(Some("guide.MD")),
    )
    .await
    .expect("upper memo should save");

    assert_eq!(lower.raw(), "lower memo");
    assert_eq!(upper.raw(), "upper memo");
    assert!(dir.path().join(".guide.md.memo.md").exists());
    assert!(dir.path().join(".guide.MD.memo.md").exists());
}

#[tokio::test]
async fn test_save_route_memo_長いファイル名でも短縮sidecarへ保存できる() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = dir.path().join(&file_name);
    fs::write(&file_path, "# long").unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("long filename should save to shortened sidecar");

    assert_eq!(memo.raw(), "memo");
    let mut memo_entries = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".memo.md"))
        .collect::<Vec<_>>();
    memo_entries.sort();
    assert_eq!(memo_entries.len(), 1);
    assert!(memo_entries[0].len() <= 255);
}

#[tokio::test]
async fn test_load_route_memo_長いファイル名で未作成なら空メモを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = dir.path().join(&file_name);
    fs::write(&file_path, "# long").unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("long filename should not break empty memo read");

    assert_eq!(memo.raw(), "");
    assert_eq!(memo.html().as_str(), "");
}

#[tokio::test]
async fn test_save_route_memo_長いファイル名のlegacyメモは空白保存で削除できる() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = format!("{}.md", "a".repeat(251));
    let file_path = dir.path().join(&file_name);
    fs::write(&file_path, "# long").unwrap();
    fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    let legacy_path = dir.path().join(".markdown-view/memos").join(&file_name);
    fs::write(&legacy_path, "memo").unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        " \n ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("legacy fallback memo should be deletable");

    assert_eq!(memo.raw(), "");
    assert!(!legacy_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_非utf8ファイル名でもsidecarが衝突しない() {
    let dir = tempfile::tempdir().unwrap();
    let lower_name = std::ffi::OsStr::from_bytes(b"guide-\xff.md");
    let upper_name = std::ffi::OsStr::from_bytes(b"guide-\xfe.md");
    let lower_path = dir.path().join(lower_name);
    let upper_path = dir.path().join(upper_name);
    fs::write(&lower_path, "# lower").unwrap();
    fs::write(&upper_path, "# upper").unwrap();

    let lower_state = create_single_file_state(&lower_path);
    let upper_state = create_single_file_state(&upper_path);
    let lower_target =
        resolve_route_target(&lower_state, RouteTargetRequest::api_memo(None)).unwrap();
    let upper_target =
        resolve_route_target(&upper_state, RouteTargetRequest::api_memo(None)).unwrap();

    let lower = save_route_memo(
        &lower_state,
        &lower_target,
        "lower memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("lower memo should save");
    let upper = save_route_memo(
        &upper_state,
        &upper_target,
        "upper memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("upper memo should save");

    assert_eq!(lower.raw(), "lower memo");
    assert_eq!(upper.raw(), "upper memo");

    let mut memo_entries = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".memo.md"))
        .collect::<Vec<_>>();
    memo_entries.sort();
    assert_eq!(memo_entries.len(), 2);
    assert_ne!(memo_entries[0], memo_entries[1]);
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_正規化される短いファイル名でもsidecarが衝突しない() {
    let dir = tempfile::tempdir().unwrap();
    let plain_path = dir.path().join("a_b.md");
    let separator_shaped_path = dir.path().join("a\\b.md");
    fs::write(&plain_path, "# plain").unwrap();
    fs::write(&separator_shaped_path, "# separator shaped").unwrap();

    let plain_state = create_single_file_state(&plain_path);
    let separator_shaped_state = create_single_file_state(&separator_shaped_path);
    let plain_target =
        resolve_route_target(&plain_state, RouteTargetRequest::api_memo(None)).unwrap();
    let separator_shaped_target =
        resolve_route_target(&separator_shaped_state, RouteTargetRequest::api_memo(None)).unwrap();

    save_route_memo(
        &plain_state,
        &plain_target,
        "plain memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("plain memo should save");
    save_route_memo(
        &separator_shaped_state,
        &separator_shaped_target,
        "separator shaped memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("separator shaped memo should save");

    let plain = load_route_memo(
        &plain_state,
        &plain_target,
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("plain memo should load");
    let separator_shaped = load_route_memo(
        &separator_shaped_state,
        &separator_shaped_target,
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("separator shaped memo should load");

    assert_eq!(plain.raw(), "plain memo");
    assert_eq!(separator_shaped.raw(), "separator shaped memo");

    let mut memo_entries = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".memo.md"))
        .collect::<Vec<_>>();
    memo_entries.sort();
    assert_eq!(memo_entries.len(), 2);
    assert!(memo_entries.contains(&".a_b.md.memo.md".to_string()));
    assert_ne!(memo_entries[0], memo_entries[1]);
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_旧形式backslash_sidecarを読み込む() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(dir.path().join(".a\\b.md.memo.md"), "compat memo").unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("compat sidecar memo should load");

    assert_eq!(memo.raw(), "compat memo");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_旧形式backslash_sidecarを新形式へ移行する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let old_sidecar = dir.path().join(".a\\b.md.memo.md");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar = dir.path().join(new_sidecar_name.as_str());
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(&old_sidecar, "compat memo").unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("compat sidecar should migrate on save");

    assert_eq!(memo.raw(), "new memo");
    assert_eq!(fs::read_to_string(&new_sidecar).unwrap(), "new memo");
    assert!(!old_sidecar.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_新旧backslash_sidecar両方ある場合は新形式を優先する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let old_sidecar = dir.path().join(".a\\b.md.memo.md");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar = dir.path().join(new_sidecar_name.as_str());
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(&old_sidecar, "compat memo").unwrap();
    fs::write(&new_sidecar, "new memo").unwrap();

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("new sidecar memo should win");

    assert_eq!(memo.raw(), "new memo");
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_sidecar優先_compat_legacy両方存在しても新sidecarを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar = dir.path().join(new_sidecar_name.as_str());
    let compat_sidecar = dir.path().join(".a\\b.md.memo.md");
    let legacy_path = dir.path().join(".markdown-view/memos/a\\b.md");
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(&new_sidecar, "new memo").unwrap();
    fs::write(&compat_sidecar, "compat memo").unwrap();
    fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    fs::write(&legacy_path, "legacy memo").unwrap();

    let state = create_directory_state(dir.path());
    let target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(Some("a\\b.md"))).unwrap();

    let memo = load_route_memo(
        &state,
        &target,
        RouteTargetRequest::api_memo(Some("a\\b.md")),
    )
    .await
    .expect("new sidecar memo should win over compat and legacy");

    assert_eq!(memo.raw(), "new memo");
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_compat優先_legacy存在でも新compatを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let compat_sidecar = dir.path().join(".a\\b.md.memo.md");
    let legacy_path = dir.path().join(".markdown-view/memos/a\\b.md");
    fs::write(&file_path, "# separator shaped").unwrap();
    fs::write(&compat_sidecar, "compat memo").unwrap();
    fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    fs::write(&legacy_path, "legacy memo").unwrap();

    let state = create_directory_state(dir.path());
    let target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(Some("a\\b.md"))).unwrap();

    let memo = load_route_memo(
        &state,
        &target,
        RouteTargetRequest::api_memo(Some("a\\b.md")),
    )
    .await
    .expect("compat sidecar memo should win over legacy");

    assert_eq!(memo.raw(), "compat memo");
}

#[tokio::test]
async fn test_load_route_memo_非utf8メモは422を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    fs::write(
        workspace.path().join(".note.md.memo.md"),
        [0xff, 0xfe, 0xfd],
    )
    .expect("non-utf8 memo sidecar should be written");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None)).await;

    let (status, body) = result.expect_err("non-utf8 memo should be rejected");
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモはUTF-8テキストである必要があります");
}

#[tokio::test]
async fn test_load_route_memo_全て不在なら空メモ() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("missing memo files should load as empty memo");

    assert_eq!(memo.raw(), "");
    assert_eq!(memo.html().as_str(), "");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_単一ファイルモードでpermission_deniedなら500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::Write,
        &sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("single file mode should not fall back");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
}

#[tokio::test]
async fn test_save_route_memo_sidecar書込不可で500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::Write,
        &sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("sidecar write failure should not fall back");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
}

#[tokio::test]
async fn test_save_route_memo_create_dir_all失敗で500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_parent = workspace.path().to_path_buf();

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::CreateDirAll,
        &sidecar_parent,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("sidecar parent creation failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
}

#[tokio::test]
#[allow(non_snake_case)]
async fn test_save_route_memo_disk_full系IO失敗で500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(Op::Write, &sidecar_path, std::io::ErrorKind::Other);
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("sidecar io failure should not fall back");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_保存成功後のcompat削除失敗は200を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("a\\b.md"), "# separator shaped")
        .expect("target markdown should be written");
    let compat_sidecar_path = workspace
        .write_file(Path::new(".a\\b.md.memo.md"), "compat memo")
        .expect("compat sidecar memo should be written");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar_path = workspace.path().join(new_sidecar_name.as_str());

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        &compat_sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs.clone());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let saved = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("compat cleanup failure should be non-fatal");

    assert_eq!(saved.raw(), "new memo");
    assert_eq!(
        memo_fs.writes().await,
        vec![(new_sidecar_path.clone(), b"new memo".to_vec())]
    );
    assert_eq!(fs::read_to_string(&new_sidecar_path).unwrap(), "new memo");
    assert!(compat_sidecar_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_compatと新sidecarが同一パスならcleanupで削除しない() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_name = format!("{}\\tail-tail-tail.md", "a".repeat(229));
    let file_path = workspace
        .write_md(Path::new(&file_name), "# separator shaped")
        .expect("target markdown should be written");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let compat_sidecar_name =
        SidecarMemoName::compat_from_file_name(std::ffi::OsStr::new(&file_name))
            .expect("backslash name should have compat sidecar");
    assert_eq!(new_sidecar_name.as_str(), compat_sidecar_name.as_str());
    let new_sidecar_path = workspace.path().join(new_sidecar_name.as_str());

    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let saved = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("same compat sidecar path should not delete active sidecar");

    assert_eq!(saved.raw(), "new memo");
    assert_eq!(fs::read_to_string(&new_sidecar_path).unwrap(), "new memo");
}

#[tokio::test]
async fn test_save_route_memo_空保存_sidecarが既にない場合は冪等的に200を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        " \n\t ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("blank save without sidecar should be idempotent");

    assert_eq!(memo.raw(), "");
    assert_eq!(memo.html().as_str(), "");
    assert!(!sidecar_path.exists());
}

#[tokio::test]
async fn test_save_route_memo_空保存_sidecar削除失敗は500を返す() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".note.md.memo.md"), "memo")
        .expect("sidecar memo should be written");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        &sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        " \n\t ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("sidecar delete failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
}

#[tokio::test]
async fn test_load_initial_socket_update_単一ファイルモードでupdateを返す() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);

    let update = load_initial_socket_update(&state).await.unwrap().unwrap();
    assert!(update.content().as_str().contains("title"));
}

#[tokio::test]
async fn test_load_initial_socket_update_ディレクトリモードではnoneを返す() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let result = load_initial_socket_update(&state).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_load_initial_socket_update_単一ファイル削除時は1008エラーを返す() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);
    std::fs::remove_file(&file_path).unwrap();

    let err = load_initial_socket_update(&state)
        .await
        .expect_err("削除済みファイルはSocketInitErrorを返すべき");

    assert_eq!(err.close_code(), 1008);
}

#[tokio::test]
async fn test_load_initial_socket_update_単一ファイルサイズ超過時は1009エラーを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("large.md");
    tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);

    let err = load_initial_socket_update(&state)
        .await
        .expect_err("サイズ超過ファイルはSocketInitErrorを返すべき");

    assert_eq!(err.close_code(), 1009);
}

#[tokio::test]
async fn test_load_route_update_ioエラーを500へ変換する() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::page(None)).unwrap();
    std::fs::remove_file(&file_path).unwrap();

    let (status, body) = load_route_update(&target, RouteTargetRequest::page(None))
        .await
        .expect_err("missing file should map to api error");

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "ファイルの読み込みに失敗しました");
}

#[tokio::test]
async fn test_load_route_update_サイズ超過を413へ変換する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("large.md");
    tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::page(None)).unwrap();

    let (status, body) = load_route_update(&target, RouteTargetRequest::page(None))
        .await
        .expect_err("oversized file should map to api error");

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "ファイルサイズが上限（10MB）を超えています");
}

#[tokio::test]
async fn test_load_route_update_非utf8を422へ変換する() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("binary.md");
    tokio::fs::write(&file_path, vec![0xff, 0xfe, 0xfd])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);
    let target = resolve_route_target(&state, RouteTargetRequest::page(None)).unwrap();

    let (status, body) = load_route_update(&target, RouteTargetRequest::page(None))
        .await
        .expect_err("invalid utf8 should map to api error");

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "このファイルはUTF-8テキストではありません");
}

#[tokio::test]
async fn test_build_lagged_recovery_message_ディレクトリモードではrefreshを返す() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());

    let message = build_lagged_recovery_message(&state).await;

    assert!(matches!(message, BroadcastMessage::Refresh));
}

#[tokio::test]
async fn test_build_lagged_recovery_message_単一ファイルモードではmemo_refresh付きupdateを返す() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# title");
    let state = create_single_file_state(&file_path);

    let message = build_lagged_recovery_message(&state).await;

    match message {
        BroadcastMessage::LaggedRecovery(message) => {
            let json = serde_json::to_value(message).unwrap();
            assert!(json["content"].as_str().unwrap().contains("title"));
            assert_eq!(json["memo_refresh"], true);
            assert!(json.get("memo_file").is_none());
        }
        other => panic!("LaggedRecoveryを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_lagged_recovery_message_単一ファイル読み込み失敗時はerrorを返す() {
    let (_dir, file_path) = create_markdown_fixture("missing.md", "# title");
    let state = create_single_file_state(&file_path);
    std::fs::remove_file(&file_path).unwrap();

    let message = build_lagged_recovery_message(&state).await;

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(msg.contains("ファイル検証エラー"));
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_lagged_recovery_message_サイズ超過時は読み込みエラーを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("large.md");
    tokio::fs::write(&file_path, vec![b'a'; (MAX_FILE_SIZE + 1) as usize])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);

    let message = build_lagged_recovery_message(&state).await;

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(
                msg.contains("ファイル読み込みエラー"),
                "ReadFailed分岐のプレフィックスを期待: {}",
                msg
            );
            assert!(
                msg.contains("ファイルサイズが上限（10MB）を超えています"),
                "TooLargeのuser_messageを期待: {}",
                msg
            );
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_lagged_recovery_message_非utf8時は読み込みエラーを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("binary.md");
    tokio::fs::write(&file_path, vec![0xff, 0xfe, 0xfd])
        .await
        .unwrap();
    let state = create_single_file_state(&file_path);

    let message = build_lagged_recovery_message(&state).await;

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(
                msg.contains("ファイル読み込みエラー"),
                "ReadFailed分岐のプレフィックスを期待: {}",
                msg
            );
            assert!(
                msg.contains("このファイルはUTF-8テキストではありません"),
                "NotUtf8のuser_messageを期待: {}",
                msg
            );
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_change_broadcast_message_ディレクトリモードでfileを含むupdateを返す() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());
    let target = dir.path().join("docs/api.md");

    let message = build_change_broadcast_message(&state, &target)
        .await
        .unwrap();
    match message {
        BroadcastMessage::Update(update) => {
            assert_eq!(update.file(), Some("docs/api.md"));
        }
        other => panic!("Updateを期待したが {:?} を受信", other),
    }
}

#[test]
fn test_revalidate_single_file_target_正常なファイルを許可する() {
    let (dir, file_path) = create_markdown_fixture("test.md", "# test");
    let canonical = file_path.canonicalize().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &base_dir);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), canonical);
}

#[test]
fn test_revalidate_single_file_target_存在しないファイルはnotfoundを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("nonexistent.md");
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&file_path, &base_dir);
    assert_eq!(result, Err(ResolveFileError::NotFound));
}

#[test]
fn test_revalidate_single_file_target_ディレクトリはnotfoundを返す() {
    let dir = tempfile::tempdir().unwrap();
    let canonical = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &canonical);
    assert_eq!(result, Err(ResolveFileError::NotFound));
}

#[test]
fn test_revalidate_single_file_target_非mdファイルはnotmarkdownを返す() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "hello").unwrap();
    let canonical = file_path.canonicalize().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&canonical, &base_dir);
    assert_eq!(result, Err(ResolveFileError::NotMarkdown));
}

#[cfg(unix)]
#[test]
fn test_revalidate_single_file_target_シンボリックリンクはtraversalを返す() {
    let dir = tempfile::tempdir().unwrap();
    let real_file = dir.path().join("real.md");
    std::fs::write(&real_file, "# real").unwrap();
    let link_path = dir.path().join("link.md");
    std::os::unix::fs::symlink(&real_file, &link_path).unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let result = revalidate_single_file_target(&link_path, &base_dir);
    assert_eq!(result, Err(ResolveFileError::Traversal));
}

fn create_test_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# README").unwrap();
    std::fs::write(dir.path().join("guide.md"), "# Guide").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "text file").unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/api.md"), "# API").unwrap();
    std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
    std::fs::write(dir.path().join(".hidden/secret.md"), "# Secret").unwrap();
    std::fs::write(dir.path().join(".dotfile.md"), "# Dot").unwrap();
    dir
}

fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join(name);
    std::fs::write(&file_path, content).unwrap();
    (dir, file_path)
}

fn create_single_file_state(file_path: &std::path::Path) -> AppState {
    let mode = AppMode::new_single_file(file_path).unwrap();
    let (tx, _rx) = broadcast::channel(4);
    AppState::new(mode, false, None, tx)
}

fn create_directory_state(dir_path: &std::path::Path) -> AppState {
    let mode = AppMode::new_directory(dir_path).unwrap();
    let (tx, _rx) = broadcast::channel(4);
    AppState::new(mode, false, None, tx)
}
