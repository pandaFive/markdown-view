use std::os::unix::ffi::OsStringExt;

use super::support::{
    create_directory_state, create_markdown_fixture, create_single_file_state, create_test_dir,
    make_dir_unsearchable,
};
use crate::server::files::*;
use crate::server::BroadcastMessage;

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

#[tokio::test]
async fn test_build_change_broadcast_message_削除済みディレクトリ変更はbroadcastをスキップする() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("deleted.md");
    std::fs::write(&target, "# deleted").unwrap();
    let state = create_directory_state(dir.path());
    std::fs::remove_file(&target).unwrap();

    let message = build_change_broadcast_message(&state, &target).await;

    assert!(message.is_none(), "削除済みファイルはbroadcastしない");
}

#[tokio::test]
async fn test_build_change_broadcast_message_削除済み単一ファイルは検証エラーをbroadcastする() {
    let (_dir, file_path) = create_markdown_fixture("deleted.md", "# deleted");
    let state = create_single_file_state(&file_path);

    std::fs::remove_file(&file_path).unwrap();
    let message = build_change_broadcast_message(&state, &file_path)
        .await
        .expect("単一ファイルの削除は検証エラーとしてbroadcastする");

    match message {
        BroadcastMessage::Error(message) => {
            assert!(message.contains("ファイル検証エラー"));
            assert!(message.contains("ファイルが見つかりません"));
        }
        other => panic!("Errorメッセージを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_change_broadcast_message_mdディレクトリは検証エラーをbroadcastする() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("folder.md");
    std::fs::create_dir(&target).unwrap();
    let state = create_directory_state(dir.path());

    let message = build_change_broadcast_message(&state, &target)
        .await
        .expect("通常ファイルでない更新対象は検証エラーをbroadcastする");

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(
                msg.contains("ファイル検証エラー"),
                "検証エラーのprefixを期待: {}",
                msg
            );
            assert!(
                msg.contains("通常ファイルではありません"),
                "NotFileのエラー文言を期待: {}",
                msg
            );
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}
#[tokio::test]
async fn test_build_change_broadcast_message_unixのbackslashファイル名をupdateに保持する() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("back\\slash.md");
    std::fs::write(&target, "# backslash").unwrap();
    let state = create_directory_state(dir.path());

    let message = build_change_broadcast_message(&state, &target)
        .await
        .expect("backslash file name should broadcast update");

    match message {
        BroadcastMessage::Update(update) => {
            assert_eq!(update.file(), Some("back\\slash.md"));
        }
        other => panic!("Updateを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_change_broadcast_message_隠しパスは検証エラーをbroadcastする() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());
    let hidden = dir.path().join(".hidden/secret.md");

    let message = build_change_broadcast_message(&state, &hidden)
        .await
        .expect("hidden path should broadcast a validation error");

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(
                msg.contains("ファイル検証エラー"),
                "検証エラーのprefixを期待: {}",
                msg
            );
            assert!(
                msg.contains("隠しファイルまたは除外対象へのアクセスは禁止されています"),
                "Hiddenのエラー文言を期待: {}",
                msg
            );
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}

#[tokio::test]
async fn test_build_change_broadcast_message_正規化不能なpathはbroadcastをスキップする() {
    let base_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside = outside_dir.path().join("missing.md");
    let state = create_directory_state(base_dir.path());

    let message = build_change_broadcast_message(&state, &outside).await;

    assert!(message.is_none(), "存在しない変更pathはbroadcastしない");
}
#[tokio::test]
async fn test_build_change_broadcast_message_非utf8相対パスはbroadcastをスキップする() {
    let dir = tempfile::tempdir().unwrap();
    let file_name = std::ffi::OsString::from_vec(b"invalid-\xff.md".to_vec());
    let target = dir.path().join(file_name);
    std::fs::write(&target, "# invalid").unwrap();
    let state = create_directory_state(dir.path());

    let message = build_change_broadcast_message(&state, &target).await;

    assert!(
        message.is_none(),
        "watcher由来の非UTF-8 pathはbroadcastしない"
    );
}
#[tokio::test]
async fn test_build_change_broadcast_message_正規化io失敗はkind付き検証エラーをbroadcastする() {
    let base_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let locked_dir = outside_dir.path().join("locked");
    std::fs::create_dir(&locked_dir).unwrap();
    let target = locked_dir.join("secret.md");
    std::fs::write(&target, "# secret").unwrap();
    let state = create_directory_state(base_dir.path());
    let Some(_guard) = make_dir_unsearchable(&locked_dir, &target) else {
        return;
    };

    let message = build_change_broadcast_message(&state, &target)
        .await
        .expect("I/O失敗は検証エラーとしてbroadcastする");

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(
                msg.contains("ファイル検証エラー"),
                "検証エラーのprefixを期待: {}",
                msg
            );
            assert!(
                msg.contains("PermissionDenied"),
                "I/O種別の表示を期待: {}",
                msg
            );
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}
#[tokio::test]
async fn test_build_change_broadcast_message_base配下の正規化io失敗はkind付き検証エラーをbroadcastする(
) {
    let dir = tempfile::tempdir().unwrap();
    let locked_dir = dir.path().join("locked");
    std::fs::create_dir(&locked_dir).unwrap();
    let target = locked_dir.join("secret.md");
    std::fs::write(&target, "# secret").unwrap();
    let state = create_directory_state(dir.path());
    let Some(_guard) = make_dir_unsearchable(&locked_dir, &target) else {
        return;
    };

    let message = build_change_broadcast_message(&state, &target)
        .await
        .expect("base配下のI/O失敗は検証エラーとしてbroadcastする");

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(
                msg.contains("ファイル検証エラー"),
                "検証エラーのprefixを期待: {}",
                msg
            );
            assert!(
                msg.contains("PermissionDenied"),
                "I/O種別の表示を期待: {}",
                msg
            );
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}
