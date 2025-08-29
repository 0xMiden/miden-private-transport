mod common;

use miden_objects::note::NoteTag;
use miden_private_transport_client::types::{
    NoteStatus, mock_note_p2id_with_accounts, mock_note_p2id_with_tag_and_accounts,
};

use self::common::*;

#[tokio::test]
async fn test_transport_client_note_fetch_tracking()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let port = 9728;
    let handle = spawn_test_server(port).await;

    let (mut client0, accid0) = test_client(port).await;
    let (mut client1, accid1) = test_client(port).await;

    let tag = TAG_LOCALANY.into();

    // Create and send a note
    let note = mock_note_p2id_with_tag_and_accounts(tag, accid0, accid1);
    let (note_id, status) = client0.send_note(note, &accid1).await.unwrap();
    assert!(matches!(status, NoteStatus::Sent));

    // Test note fetching recording

    // Initially, the note should not be marked as fetched
    assert!(!client1.note_fetched(&note_id).await.unwrap());

    let _notes = client1.fetch_notes(tag).await?;
    assert!(client1.note_fetched(&note_id).await.unwrap());

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_transport_client_note_storage() -> std::result::Result<(), Box<dyn std::error::Error>>
{
    let port = 9730;
    let handle = spawn_test_server(port).await;

    let (mut client0, accid0) = test_client(port).await;
    let (mut client1, accid1) = test_client(port).await;

    // Send a note
    let note = mock_note_p2id_with_accounts(accid0, accid1);
    let (note_id, note_status) = client0.send_note(note, &accid1).await.unwrap();
    assert!(matches!(note_status, NoteStatus::Sent));

    // Fetch
    let sent_tag = NoteTag::from_account_id(accid1);
    let fetched_notes = client1.fetch_notes(sent_tag).await.unwrap();
    assert_eq!(fetched_notes.len(), 1);

    // Verify marked as fetched in the DB
    let fetched_note_ids = client1.get_fetched_notes_for_tag(sent_tag).await.unwrap();
    assert_eq!(fetched_note_ids.len(), 1);
    assert_eq!(fetched_note_ids[0], note_id);

    // Verify database statistics
    let stats = client1.get_database_stats().await.unwrap();
    assert_eq!(stats.fetched_notes_count, 1);
    assert_eq!(stats.stored_notes_count, 1);
    assert_eq!(stats.unique_tags_count, 1);

    handle.abort();
    Ok(())
}
