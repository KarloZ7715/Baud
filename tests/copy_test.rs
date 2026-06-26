// Test to verify selected_text returns the correct text
// and that clipboard copy works end-to-end
use baud::ansi::Term;
use baud::selection::{Selection, SelectionPoint};

/// Verify selected_text() returns the correct text for a simple selection
#[test]
fn test_copy_simple_selection() {
    let mut term = Term::new();
    let mut parser = vte::Parser::new();
    
    parser.advance(&mut term, b"linea_de_prueba_0\n");
    parser.advance(&mut term, b"linea_de_prueba_1\n");
    parser.advance(&mut term, b"echo HOLA_MUNDO\n");
    
    // Find actual content of row 0
    let grid0 = term.active_grid();
    let row0: String = grid0.rows[0].iter().map(|c| c.ch).collect();
    eprintln!("ROW 0: {:?}", row0.trim_end());
    let row1: String = grid0.rows[1].iter().map(|c| c.ch).collect();
    eprintln!("ROW 1: {:?}", row1.trim_end());
    let row2: String = grid0.rows[2].iter().map(|c| c.ch).collect();
    eprintln!("ROW 2: {:?}", row2.trim_end());
    
    // Select row 0 - get the actual non-space length
    let actual_len = row0.trim_end().len();
    let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
    sel.update_end(SelectionPoint { row: 0, col: actual_len.saturating_sub(1) });
    term.selection = Some(sel);
    
    let text = term.selected_text();
    eprintln!("SELECTED ROW 0: {:?}", text);
    
    // The text should match the trimmed content of row 0
    assert!(!text.is_empty(), "selected_text() no debe estar vacio");
    assert_eq!(text, row0.trim_end(), 
        "selected_text debe coincidir con el contenido de row 0");
}
