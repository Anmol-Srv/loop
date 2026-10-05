use std::cell::RefCell;
use std::rc::Rc;

use acp_server::desktop::design::widgets as w;
use egui::accesskit::{Action, ActionData, ActionRequest, Role};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

#[test]
fn a_field_is_named_by_its_caption_and_takes_a_value_from_assistive_tech() {
    let password = Rc::new(RefCell::new(String::new()));
    let shown = password.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 200.0))
        .build_ui(move |ui| {
            w::field(ui, "Password", &mut shown.borrow_mut(), true, "");
        });
    harness.run_steps(2);

    let (target_node, target_tree) = harness
        .get_by_role_and_label(Role::PasswordInput, "Password")
        .accesskit_node()
        .locate();
    harness.event(egui::Event::AccessKitActionRequest(ActionRequest {
        action: Action::SetValue,
        target_tree,
        target_node,
        data: Some(ActionData::Value("correct horse".into())),
    }));
    harness.run_steps(2);

    assert_eq!(password.borrow().as_str(), "correct horse");
}

#[test]
fn a_right_click_keeps_the_selection_so_copy_is_offered() {
    let mut text = String::from("ready to copy");
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 300.0))
        .build_ui(move |ui| {
            w::field(ui, "Note", &mut text, false, "");
        });
    harness.run_steps(2);

    harness.get_by_role_and_label(Role::TextInput, "Note").click();
    harness.run_steps(2);
    harness.key_combination_modifiers(egui::Modifiers::COMMAND, &[egui::Key::A]);
    harness.run_steps(2);
    harness.get_by_role_and_label(Role::TextInput, "Note").click_secondary();
    harness.run_steps(3);

    let copy = harness.get_by_role_and_label(Role::Button, "Copy");
    assert!(!copy.accesskit_node().is_disabled(), "Copy is offered for the kept selection");
    copy.click();
    harness.step();

    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "ready to copy"));
    assert!(copied, "Copy puts the whole selection on the clipboard");
}
