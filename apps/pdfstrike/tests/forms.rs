use std::sync::Arc;

use pdfcraft_cos::{Document, SaveOptions, write_full};
use pdfcraft_forms::{NewField, add_field, fields};
use pdfcraft_organize::insert_blank_page;
use pdfstrike::{Edit, Project};

#[test]
fn text_and_checkbox_values_have_exported_appearances() {
    let mut source = Document::new_empty();
    insert_blank_page(&mut source, 0, 612.0, 792.0).unwrap();
    add_field(
        &mut source,
        0,
        [72.0, 680.0, 300.0, 710.0],
        &NewField::Text { multiline: false },
        Some("contact_name"),
    )
    .unwrap();
    add_field(
        &mut source,
        0,
        [72.0, 630.0, 96.0, 654.0],
        &NewField::CheckBox,
        Some("approved"),
    )
    .unwrap();
    let bytes = write_full(&source, &SaveOptions::default()).unwrap();
    let mut project = Project::open_pdf(bytes).unwrap();
    project
        .apply(Edit::TextField {
            name: "contact_name".into(),
            value: "Ada Lovelace".into(),
        })
        .unwrap();
    project
        .apply(Edit::CheckBox {
            name: "approved".into(),
            checked: true,
        })
        .unwrap();
    let exported = Document::open(Arc::new(project.export_pdf().unwrap())).unwrap();
    let values = fields(&exported);
    assert_eq!(
        values
            .iter()
            .find(|f| f.name == "contact_name")
            .unwrap()
            .value,
        ["Ada Lovelace"]
    );
    assert!(
        !values
            .iter()
            .find(|f| f.name == "approved")
            .unwrap()
            .value
            .is_empty()
    );
    for field in values {
        let widget = field.widgets.first().unwrap();
        let dict = exported.get(widget.obj);
        assert!(
            dict.as_dict().unwrap().get(b"AP").is_some(),
            "{} has no appearance",
            field.name
        );
    }
}
