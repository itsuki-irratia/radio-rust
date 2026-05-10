use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Button, Label, Orientation};

pub fn run_gui() {
    let app = Application::builder()
        .application_id("dev.radiofm.scheduler")
        .build();

    app.connect_activate(|app| {
        let label = Label::new(Some("Radio FM Scheduler starter is running"));
        let button = Button::with_label("Close");
        let app_weak = app.downgrade();
        button.connect_clicked(move |_| {
            if let Some(app) = app_weak.upgrade() {
                app.quit();
            }
        });

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        container.append(&label);
        container.append(&button);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Radio FM Scheduler")
            .default_width(520)
            .default_height(220)
            .child(&container)
            .build();

        window.present();
    });

    app.run();
}
