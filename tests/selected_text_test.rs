// Test file for selected_text
// Run with: cargo test --test test_selected_text_copy
use baud::ansi::Term;
use baud::selection::{Selection, SelectionPoint};

#[test]
fn test_selected_text_hello_world() {
    let mut term = Term::new();
    let mut parser = vte::Parser::new();

    // Feed text like the user would type
    parser.advance(&mut term, b"hello world");

    // Select "hello world" (row 0, col 0 to col 10)
    let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
    sel.update_end(SelectionPoint { row: 0, col: 10 });
    term.selection = Some(sel);

    let text = term.selected_text();
    assert_eq!(
        text, "hello world",
        "selected_text() debe devolver 'hello world', obtuvo: {:?}",
        text
    );
}

#[test]
fn test_selected_text_multiline() {
    let mut term = Term::new();
    let mut parser = vte::Parser::new();

    // Feed multiple lines (PTY entrega CR+LF via ONLCR)
    parser.advance(&mut term, b"line1\r\n");
    parser.advance(&mut term, b"line2\r\n");
    parser.advance(&mut term, b"line3");

    // Select all three lines
    let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
    sel.update_end(SelectionPoint { row: 2, col: 4 });
    term.selection = Some(sel);

    let text = term.selected_text();
    assert!(
        text.contains("line1"),
        "debe contener line1, obtuvo: {:?}",
        text
    );
    assert!(
        text.contains("line2"),
        "debe contener line2, obtuvo: {:?}",
        text
    );
    assert!(
        text.contains("line3"),
        "debe contener line3, obtuvo: {:?}",
        text
    );
}

#[test]
fn test_selected_text_non_empty() {
    let mut term = Term::new();
    let mut parser = vte::Parser::new();

    parser.advance(&mut term, b"test content for clipboard");

    let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
    sel.update_end(SelectionPoint { row: 0, col: 24 });
    term.selection = Some(sel);

    let text = term.selected_text();
    assert!(!text.is_empty(), "selected_text() no debe estar vacio");
    assert_eq!(text.chars().count(), 25, "debe tener 25 caracteres");
    println!("SELECTED_TEXT: {:?}", text);
}
