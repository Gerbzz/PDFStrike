use std::path::Path;

use pdfstrike_project::Project;

#[test]
fn native_project_core_opens_cli_authored_edit_history() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../fixtures/pdfstrike/native-compat.pdfstrike");
    let mut project = Project::open_project(&path).expect("CLI-authored project opens");
    assert_eq!(project.page_count().unwrap(), 4);
    let edited = project.export_pdf().unwrap();
    project.undo().unwrap();
    assert_ne!(project.export_pdf().unwrap(), edited);
    project.redo().unwrap();
    assert_eq!(project.export_pdf().unwrap(), edited);
}
