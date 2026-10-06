#![cfg(feature = "app")]

use acp_server::desktop::menu;
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

#[test]
fn edit_menu_items_reach_the_focused_text_field() {
    let mut text = String::from("hand it to Hermes");
    let pasted = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = pasted.clone();
    let listening = std::rc::Rc::new(std::cell::Cell::new(true));
    let listen = listening.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 120.0))
        .build_ui(move |ui| {
            menu::deliver(ui.ctx());
            if listen.get() {
                seen.set(seen.get() || menu::pasted(ui.ctx()));
            }
            ui.add(egui::TextEdit::singleline(&mut text));
        });
    harness.run_steps(2);
    harness.get_by_role(Role::TextInput).click();
    harness.run_steps(2);

    for id in ["edit.select-all", "edit.copy"] {
        menu::pick(id);
        menu::route(harness.input_mut());
        harness.step();
    }
    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "hand it to Hermes"));
    assert!(copied, "Select All then Copy from the menu copies the whole field");

    menu::pick("edit.paste");
    menu::route(harness.input_mut());
    harness.step();
    let deferred = harness
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .is_some_and(|v| v.commands.contains(&egui::ViewportCommand::RequestPaste));
    assert!(!deferred, "the text is pasted in this frame, not asked for a frame or two later, so a copied file is attached once");
    assert!(pasted.get(), "and tells the page, which looks for an image or files to attach");

    pasted.set(false);
    listening.set(false);
    menu::pick("edit.paste");
    menu::route(harness.input_mut());
    harness.step();
    listening.set(true);
    harness.step();
    assert!(!pasted.get(), "a paste no page took is gone by the next frame, so a task opened later attaches nothing");
}
