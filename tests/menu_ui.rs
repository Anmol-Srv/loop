use acp_server::desktop::menu;
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

#[test]
fn edit_menu_items_reach_the_focused_text_field() {
    let mut text = String::from("hand it to Hermes");
    let pasted = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = pasted.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 120.0))
        .build_ui(move |ui| {
            menu::deliver(ui.ctx());
            seen.set(seen.get() || menu::pasted(ui.ctx()));
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
    let asked = harness
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .is_some_and(|v| v.commands.contains(&egui::ViewportCommand::RequestPaste));
    assert!(asked, "Paste asks the window for the clipboard");
    assert!(pasted.get(), "and tells the page, which looks for an image or files to attach");
}
