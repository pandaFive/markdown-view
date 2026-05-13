use super::support::{
    create_directory_state, create_markdown_fixture, create_single_file_state, create_test_dir,
};
use crate::server::files::*;
use crate::server::BroadcastMessage;

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
