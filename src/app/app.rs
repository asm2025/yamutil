use gtk::prelude::*;
use gtk::{
    glib::clone, Application, ApplicationWindow, Box, Button, Entry, Grid, Label, Orientation,
};
use relm::prelude::*;
use rustmix::*;
use std::sync::Arc;

use super::*;
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
        let application = Application::builder().application_id(APP_ID).build();
        let appinfo = self.appinfo.clone();
        application.connect_activate(clone!(
            #[strong]
            appinfo,
            move |app| {
                Self::build_ui(app, &appinfo);
            }
        ));

        let args: Vec<&str> = Vec::new();
        let exit_code = application.run_with_args(&args);
        match exit_code.value() {
            0 => Ok(0),
            it => Err(ExitCodeError(it).into()),
        }
    }

    fn build_ui<'a>(app: &Application, appinfo: &Arc<AppInfo<'a>>) {
        let grid = Grid::builder()
            .margin_top(CTRL_MARGIN)
            .margin_bottom(CTRL_MARGIN)
            .margin_start(CTRL_MARGIN)
            .margin_end(CTRL_MARGIN)
            .row_spacing(CTRL_SPACING)
            .column_spacing(CTRL_SPACING)
            .build();
        grid.set_orientation(Orientation::Horizontal);

        let lbl_token = Label::builder().label("Token: ").width_request(40).build();
        let ent_token = Entry::builder().hexpand(true).build();
        let btn_token = Button::builder().label("Set").width_request(24).build();
        btn_token.connect_clicked(|e| {
            e.set_label("Thanks!");
        });
        grid.attach(&lbl_token, 0, 0, 1, 1);
        grid.attach(&ent_token, 1, 0, 1, 1);
        grid.attach(&btn_token, 2, 0, 1, 1);

        let window = ApplicationWindow::builder()
            .application(app)
            .title(&format!("{} v{}", appinfo.name, appinfo.version))
            .default_width(640)
            .default_height(480)
            .child(&grid)
            .build();

        // last
        window.present();
    }
}
