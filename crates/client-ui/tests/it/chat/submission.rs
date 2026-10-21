//! Chat editor submissions preserve text after their leading ASCII whitespace.

use super::*;

#[test]
fn editor_submission_removes_ascii_prefix_before_command_routing() {
    for prefix in [" ", "\t", "\n", "\u{b}", "\u{c}", "\r", " \t\r\n"] {
        let mut runtime = UiRuntime::new(1);
        runtime
            .insert_chat_text(&format!("{prefix}/help  "))
            .unwrap();
        let request = runtime.queue_chat_send(0).unwrap();
        assert_eq!(request.message.as_ref(), "/help  ");
        let (_, packet) = runtime.next_chat_packet().unwrap().unwrap();
        assert_eq!(
            packet.kind(),
            protocol::command_request_packet("/help  ").kind()
        );
        assert!(runtime.show_older_chat_history());
        assert_eq!(runtime.chat_editor().as_str(), "/help  ");
    }
}

#[test]
fn editor_submission_keeps_trailing_and_non_ascii_whitespace() {
    for (input, expected) in [
        ("  hello  ", "hello  "),
        ("\u{a0}/help", "\u{a0}/help"),
        ("\u{2003}hello", "\u{2003}hello"),
    ] {
        let mut runtime = UiRuntime::new(1);
        runtime.set_chat_identity(Arc::from("Reader"), Arc::from(""));
        runtime.insert_chat_text(input).unwrap();
        let request = runtime.queue_chat_send(0).unwrap();
        assert_eq!(request.message.as_ref(), expected);
        let (_, packet) = runtime.next_chat_packet().unwrap().unwrap();
        assert_eq!(packet, chat_text_packet("Reader", "", expected).unwrap());
    }
}

#[test]
fn whitespace_only_editor_submission_does_not_enqueue_a_packet() {
    let mut runtime = UiRuntime::new(1);
    runtime.insert_chat_text(" \t\r\n\u{b}\u{c}").unwrap();
    assert!(matches!(
        runtime.queue_chat_send(0),
        Err(ui::ChatSendError::EmptyMessage)
    ));
    assert!(runtime.pending_chat_sends().is_empty());
    assert!(runtime.next_chat_packet().unwrap().is_none());
}

#[test]
fn explicit_message_queue_retains_its_exact_text_contract() {
    let mut runtime = UiRuntime::new(1);
    let request = runtime.queue_chat_message("  hello  ", 0).unwrap();
    assert_eq!(request.message.as_ref(), "  hello  ");
}
