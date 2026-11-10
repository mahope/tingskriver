use super::*;
use crate::api::{MeetingDetails, MeetingTranscript};

#[tokio::test]
async fn export_meeting_renders_both_formats_without_replacing_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut meeting = meeting();
    meeting.title = "Planning/next".into();
    let data = ExportData {
        meeting: &meeting,
        duration_seconds: Some(60),
        speakers: &[],
        language: Some("da"),
        summary: "<summary>",
        notes: "Notes",
    };
    let existing = dir.path().join("2026-01-02 Planning_next.md");
    tokio::fs::write(&existing, "keep me").await.unwrap();
    for (format, expected_name, expected_content) in [
        (
            Format::Markdown,
            "2026-01-02 Planning_next (2).md",
            render_markdown(&data),
        ),
        (
            Format::Html,
            "2026-01-02 Planning_next.html",
            render_html(&data),
        ),
    ] {
        let path = export_meeting(dir.path(), "{dato} {titel}", format, &data)
            .await
            .unwrap();
        assert_eq!(path.file_name().unwrap(), expected_name);
        assert_eq!(
            tokio::fs::read_to_string(path).await.unwrap(),
            expected_content
        );
    }
    assert_eq!(
        tokio::fs::read_to_string(existing).await.unwrap(),
        "keep me"
    );
}

fn meeting() -> MeetingDetails {
    MeetingDetails {
        id: "fixture".into(),
        title: "Title: \"quoted\"\n---".into(),
        created_at: "2026-01-02T03:04:05Z".into(),
        updated_at: "2026-01-02T03:04:05Z".into(),
        transcripts: vec![MeetingTranscript {
            id: "segment".into(),
            text: "<script>&\"'".into(),
            timestamp: "00:01".into(),
            audio_start_time: Some(1.0),
            audio_end_time: Some(2.0),
            duration: Some(1.0),
        }],
    }
}

#[test]
fn frontmatter_and_sections_preserve_content() {
    let meeting = meeting();
    let data = ExportData {
        meeting: &meeting,
        duration_seconds: Some(60),
        speakers: &["Speaker: 1".into()],
        language: Some("da"),
        summary: "Summary",
        notes: "Notes",
    };
    let text = render_markdown(&data);
    assert!(text.starts_with("---\ntitle: \"Title: \\\"quoted\\\"\\n---\"\n"));
    assert!(text.contains("date: \"2026-01-02T03:04:05Z\"\nduration_seconds: 60\nspeakers: [\"Speaker: 1\"]\nlanguage: \"da\"\n---\n"));
    for part in [
        "## Referat\n\nSummary",
        "## Noter\n\nNotes",
        "## Transskript",
        "<script>&\"'",
    ] {
        assert!(text.contains(part), "{part}");
    }
}

#[test]
fn html_escapes_all_dynamic_content() {
    let mut meeting = meeting();
    meeting.title = "<img src=x>".into();
    let data = ExportData {
        meeting: &meeting,
        duration_seconds: None,
        speakers: &["<speaker>".into()],
        language: Some("<da>"),
        summary: "<summary>",
        notes: "<notes>",
    };
    let html = render_html(&data);
    assert!(html.starts_with("<!doctype html>"));
    for part in [
        "&lt;img src=x&gt;",
        "&lt;script&gt;&amp;&quot;&#39;",
        "&lt;speaker&gt;",
        "&lt;da&gt;",
        "&lt;summary&gt;",
        "&lt;notes&gt;",
    ] {
        assert!(html.contains(part), "{part}");
    }
    assert!(!html.contains("<script>"));
    assert!(!html.contains("<img"));
}

#[test]
fn portable_filenames() {
    for name in [
        "CON", "con.txt", "PRN", "AUX", "NUL", "COM1", "LPT9", "COM¹", "LPT²",
    ] {
        assert!(sanitize_filename(name).starts_with('_'), "{name}");
    }
    assert_eq!(sanitize_filename("a<>:\"/\\|?*b. "), "a_________b");
    assert_eq!(sanitize_filename(".."), "meeting");
    assert_eq!(sanitize_filename("\0\n"), "__");
    assert_eq!(
        filename("{dato} {titel}", "2026/01/02", "../Title"),
        "2026_01_02 .._Title"
    );
    assert!(sanitize_filename(&"ø".repeat(300)).encode_utf16().count() <= 180);
    assert!(sanitize_filename(&"😀".repeat(300)).len() <= 180);
    assert_eq!(filename("{titel}", "date", "{dato}"), "{dato}");
}

#[test]
fn absent_metadata_and_empty_sections_are_explicit() {
    let mut meeting = meeting();
    meeting.transcripts.clear();
    let data = ExportData {
        meeting: &meeting,
        duration_seconds: None,
        speakers: &[],
        language: None,
        summary: "",
        notes: "",
    };
    let markdown = render_markdown(&data);
    assert!(markdown.contains("duration_seconds: null\nspeakers: []\nlanguage: null\n---"));
    assert!(markdown.ends_with("## Transskript\n\n"));
    assert!(render_html(&data).ends_with("</body></html>\n"));
}

#[test]
fn transcript_uses_recording_order_in_both_formats() {
    let mut meeting = meeting();
    let mut later = MeetingTranscript {
        id: "later".into(),
        text: "later".into(),
        timestamp: "00:03".into(),
        audio_start_time: Some(3.0),
        audio_end_time: Some(4.0),
        duration: Some(1.0),
    };
    std::mem::swap(&mut later, &mut meeting.transcripts[0]);
    meeting.transcripts.push(later);
    meeting.transcripts[1].text = "earlier".into();
    let data = ExportData {
        meeting: &meeting,
        duration_seconds: None,
        speakers: &[],
        language: None,
        summary: "",
        notes: "",
    };
    for text in [render_markdown(&data), render_html(&data)] {
        assert!(text.find("earlier").unwrap() < text.find("later").unwrap());
    }
}

#[tokio::test]
async fn missing_directory_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let error = write_document(
        &dir.path().join("missing"),
        "meeting",
        Format::Html,
        "text".into(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[tokio::test]
async fn never_overwrites_and_leaves_no_temporary_files() {
    let dir = tempfile::tempdir().unwrap();
    let first = write_document(dir.path(), "CON", Format::Markdown, "first".into())
        .await
        .unwrap();
    let second = write_document(dir.path(), "CON", Format::Markdown, "second".into())
        .await
        .unwrap();
    let third = write_document(dir.path(), "CON", Format::Markdown, "third".into())
        .await
        .unwrap();
    assert_eq!(first.file_name().unwrap(), "_CON.md");
    assert_eq!(second.file_name().unwrap(), "_CON (2).md");
    assert_eq!(third.file_name().unwrap(), "_CON (3).md");
    assert_eq!(tokio::fs::read_to_string(first).await.unwrap(), "first");
    assert_eq!(tokio::fs::read_to_string(second).await.unwrap(), "second");
    let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
    let mut count = 0;
    while entries.next_entry().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 3);
}

#[tokio::test]
async fn concurrent_exports_choose_different_paths() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = tokio::join!(
        write_document(dir.path(), "same", Format::Html, "a".into()),
        write_document(dir.path(), "same", Format::Html, "b".into())
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a, b);
    assert_eq!(tokio::fs::read_to_string(a).await.unwrap(), "a");
    assert_eq!(tokio::fs::read_to_string(b).await.unwrap(), "b");
}

#[tokio::test]
async fn existing_directory_is_preserved_and_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let existing = dir.path().join("meeting.md");
    tokio::fs::create_dir(&existing).await.unwrap();
    let path = write_document(dir.path(), "meeting", Format::Markdown, "document".into())
        .await
        .unwrap();
    assert_eq!(path.file_name().unwrap(), "meeting (2).md");
    assert!(tokio::fs::metadata(existing).await.unwrap().is_dir());
    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), "document");
    let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
    let mut count = 0;
    while entries.next_entry().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 2);
}

#[cfg(unix)]
#[tokio::test]
async fn existing_symlink_is_preserved_without_touching_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let target = target_dir.path().join("original.md");
    tokio::fs::write(&target, "original").await.unwrap();
    let link = dir.path().join("meeting.md");
    tokio::fs::symlink(&target, &link).await.unwrap();
    let path = write_document(dir.path(), "meeting", Format::Markdown, "document".into())
        .await
        .unwrap();
    assert_eq!(path.file_name().unwrap(), "meeting (2).md");
    assert_eq!(tokio::fs::read_link(link).await.unwrap(), target);
    assert_eq!(tokio::fs::read_to_string(target).await.unwrap(), "original");
    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), "document");
    let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
    let mut count = 0;
    while entries.next_entry().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 2);
}
