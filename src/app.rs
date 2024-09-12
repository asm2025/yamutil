use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Button, Label, Orientation, WindowPosition};
use rustmix::*;
use std::sync::Arc;

use crate::{error::ExitCodeError, service::*};

#[derive(Debug, Clone)]
pub struct App {
    appinfo: Arc<AppInfo<'static>>,
    service: Arc<Service>,
}

impl App {
    pub fn new(appinfo: Arc<AppInfo<'static>>, service: Arc<Service>) -> Self {
        Self { appinfo, service }
    }

    pub async fn run(&self) -> Result<i32> {
        let app_id = format!("com.github.{}.{}", self.appinfo.authors, self.appinfo.name);
        let app_title = format!("{} v{}", self.appinfo.name, self.appinfo.version);
        let application = Application::builder().application_id(app_id).build();
        //let service = self.service.clone();
        application.connect_activate(move |app| {
            let window = ApplicationWindow::builder()
                .application(app)
                .title(&app_title)
                .default_width(640)
                .default_height(480)
                .window_position(WindowPosition::Center)
                .build();
            let vbox = gtk::Box::builder()
                .orientation(Orientation::Vertical)
                .spacing(4)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(8)
                .margin_end(8)
                .build();
            let label = Label::new(Some("Enter text:"));
            let textbox = gtk::Entry::new();
            let button = Button::with_label("Submit");

            vbox.pack_start(&label, false, false, 0);
            vbox.pack_start(&textbox, false, false, 0);
            vbox.pack_start(&button, false, false, 0);

            window.add(&vbox);
            window.show_all();
        });

        let args: Vec<&str> = Vec::new();
        let exit_code = application.run_with_args(&args);
        match exit_code.value() {
            0 => Ok(0),
            it => Err(ExitCodeError(it).into()),
        }
    }
}
