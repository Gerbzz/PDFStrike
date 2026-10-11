use std::sync::Arc;

use pdfcraft_cos::Document;
use pdfcraft_forms::fields;
use pdfcraft_organize::{page_boxes, page_rotation};
use pdfstrike::{Edit, Project};

#[test]
fn independent_packet_reorders_and_keeps_crop_and_source_bytes() {
    let source = include_bytes!("../../../fixtures/pdfstrike/packet-real.pdf").to_vec();
    let mut project = Project::open_pdf(source.clone()).unwrap();
    assert_eq!(project.page_count().unwrap(), 4);
    project
        .apply(Edit::MovePages {
            indices: vec![3],
            to: 0,
        })
        .unwrap();
    project
        .apply(Edit::DeletePages { indices: vec![1] })
        .unwrap();
    project
        .apply(Edit::Rotate {
            page: 0,
            degrees: 90,
        })
        .unwrap();
    let path = std::env::temp_dir().join(format!(
        "pdfstrike-real-packet-{}.pdfstrike",
        std::process::id()
    ));
    project.save_project(&path).unwrap();
    let reopened = Project::open_project(&path).unwrap();
    let doc = Document::open(Arc::new(reopened.export_pdf().unwrap())).unwrap();
    assert_eq!(page_boxes(&doc).unwrap().len(), 3);
    assert_eq!(page_rotation(&doc, 0).unwrap(), 90);
    assert_eq!(page_rotation(&doc, 1).unwrap(), 90);
    assert_eq!(page_boxes(&doc).unwrap()[2][1], [36.0, 36.0, 576.0, 756.0]);
    assert_eq!(reopened.source_bytes(), source);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn independent_acroform_values_and_appearances_round_trip() {
    let source = include_bytes!("../../../fixtures/pdfstrike/form-real.pdf").to_vec();
    let mut project = Project::open_pdf(source).unwrap();
    project
        .apply(Edit::TextField {
            name: "contact.name".into(),
            value: "Ada Lovelace".into(),
        })
        .unwrap();
    project
        .apply(Edit::CheckBox {
            name: "approved".into(),
            checked: true,
        })
        .unwrap();
    let doc = Document::open(Arc::new(project.export_pdf().unwrap())).unwrap();
    let values = fields(&doc);
    assert_eq!(
        values
            .iter()
            .find(|f| f.name == "contact.name")
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
        assert!(
            doc.get(widget.obj).as_dict().unwrap().get(b"AP").is_some(),
            "{} lacks an appearance",
            field.name
        );
    }
}
