use gtk::prelude::*;
use gtk::{
    glib::clone, Adjustment, Align, Application, ApplicationWindow, Box, Button, CheckButton,
    Entry, Grid, Label, Orientation, SpinButton,
};
use rustmix::{error::*, *};
use std::sync::Arc;

use super::*;
use crate::{common::*, service::*};

#[derive(Debug, Clone)]
pub struct AppSettings {
    pub token: Option<String>,
    pub group_id: Option<u64>,
    pub thread_id: Option<u64>,
    pub email: Option<String>,
    pub list_threads: bool,
}

#[derive(Debug, Clone)]
pub struct App {
    pub settings: AppSettings,
    appinfo: Arc<AppInfo<'static>>,
    service: Arc<Service>,
}

impl App {
    pub fn new(
        appinfo: Arc<AppInfo<'static>>,
        service: Arc<Service>,
        token: Option<String>,
    ) -> Self {
        Self {
            settings: AppSettings {
                token,
                group_id: None,
                thread_id: None,
                email: None,
                list_threads: false,
            },
            appinfo,
            service,
        }
    }

    pub async fn run(&self) -> Result<i32> {
        let application = Application::builder().application_id(APP_ID).build();
        let appinfo = self.appinfo.clone();
        let settings = self.settings.clone();
        application.connect_activate(clone!(
            #[strong]
            appinfo,
            move |app| {
                Self::build_ui(app, &appinfo, &settings);
            }
        ));

        let args: Vec<&str> = Vec::new();
        let exit_code = application.run_with_args(&args);
        match exit_code.value() {
            0 => Ok(0),
            it => Err(ExitCodeError(it).into()),
        }
    }

    fn build_ui<'a>(app: &Application, appinfo: &Arc<AppInfo<'a>>, settings: &AppSettings) {
        let grid = Grid::builder()
            .margin_top(CTRL_MARGIN)
            .margin_bottom(CTRL_MARGIN)
            .margin_start(CTRL_MARGIN)
            .margin_end(CTRL_MARGIN)
            .row_spacing(CTRL_SPACING)
            .column_spacing(CTRL_SPACING)
            .build();
        grid.set_orientation(Orientation::Horizontal);
        let mut row = 0;

        // token row
        let lbl_token = Label::builder().label("Token: ").width_request(40).build();
        let ent_token = Entry::builder()
            .hexpand(true)
            .text(settings.token.as_ref().unwrap_or(&String::new()))
            .build();
        ent_token.connect_changed(clone!(
            #[strong]
            settings,
            move |e| {
                let mut settings = settings.clone();
                settings.token = Some(e.text().to_string());
            }
        ));
        grid.attach(&lbl_token, 0, row, 1, 1);
        grid.attach(&ent_token, 1, row, 1, 1);
        row += 1;

        // group row
        let lbl_group = Label::builder().label("Group: ").width_request(40).build();
        let adj_group = Adjustment::new(
            settings.group_id.unwrap_or(0) as f64,
            0.0,
            u64::MAX as f64,
            1.0,
            10.0,
            0.0,
        );
        let spn_group = SpinButton::builder()
            .hexpand(true)
            .adjustment(&adj_group)
            .build();
        spn_group.connect_value_changed(clone!(
            #[strong]
            settings,
            move |e| {
                let mut settings = settings.clone();
                settings.group_id = if e.value() == 0.0 {
                    None
                } else {
                    Some(e.value() as u64)
                };
            }
        ));
        grid.attach(&lbl_group, 0, row, 1, 1);
        grid.attach(&spn_group, 1, row, 1, 1);
        row += 1;

        // thread row
        let lbl_thread = Label::builder().label("Thread: ").width_request(40).build();
        let adj_thread = Adjustment::new(
            settings.thread_id.unwrap_or(0) as f64,
            0.0,
            u64::MAX as f64,
            1.0,
            10.0,
            0.0,
        );
        let spn_thread = SpinButton::builder()
            .hexpand(true)
            .adjustment(&adj_thread)
            .build();
        spn_thread.connect_changed(clone!(
            #[strong]
            settings,
            move |e| {
                let mut settings = settings.clone();
                settings.thread_id = if e.value() == 0.0 {
                    None
                } else {
                    Some(e.value() as u64)
                };
            }
        ));
        grid.attach(&lbl_thread, 0, row, 1, 1);
        grid.attach(&spn_thread, 1, row, 1, 1);
        row += 1;

        // list threads row
        let chk_list_threads = CheckButton::builder()
            .label("List Threads")
            .active(settings.list_threads)
            .build();
        chk_list_threads.connect_toggled(clone!(
            #[strong]
            settings,
            move |e| {
                let mut settings = settings.clone();
                settings.list_threads = e.is_active();
            }
        ));
        grid.attach(&chk_list_threads, 1, row, 1, 1);
        row += 1;

        // email row
        let lbl_email = Label::builder().label("Email: ").width_request(40).build();
        let ent_email = Entry::builder()
            .hexpand(true)
            .text(settings.email.as_ref().unwrap_or(&String::new()))
            .build();
        ent_email.connect_changed(clone!(
            #[strong]
            settings,
            move |e| {
                let mut settings = settings.clone();
                settings.email = Some(e.text().to_string());
            }
        ));
        grid.attach(&lbl_email, 0, row, 1, 1);
        grid.attach(&ent_email, 1, row, 1, 1);
        row += 1;

        // commands row
        let box_cmd = Box::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .halign(Align::End)
            .build();
        let btn_list = Button::builder().label("List").width_request(24).build();
        let btn_delete = Button::builder().label("Delete").width_request(24).build();
        btn_list.connect_clicked(|e| {
            e.set_label("Stop");
        });
        box_cmd.append(&btn_list);
        box_cmd.append(&btn_delete);
        grid.attach(&box_cmd, 1, row, 1, 1);

        // last
        let window = ApplicationWindow::builder()
            .application(app)
            .title(&format!("{} v{}", appinfo.name, appinfo.version))
            .default_width(640)
            .default_height(480)
            .build();
        window.set_child(Some(&grid));
        window.present();
    }
}
