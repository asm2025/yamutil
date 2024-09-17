use gtk::prelude::*;
use gtk::{glib::clone, Application, ApplicationWindow, Button, Entry, Grid, Label, Orientation};
use rustmix::{error::*, *};
use std::{collections::HashSet, sync::Arc};

use super::*;
use crate::{common::*, service::*};

#[derive(Debug, Clone)]
pub struct App {
    token: String,
    group_id: Option<u64>,
    thread_id: Option<u64>,
    email: Option<String>,
    list_threads: bool,
    exclude_from_deletion: HashSet<u64>,
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
            token: token.unwrap_or_default(),
            group_id: None,
            thread_id: None,
            email: None,
            list_threads: false,
            exclude_from_deletion: HashSet::new(),
            appinfo,
            service,
        }
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
        let window = ApplicationWindow::builder()
            .application(app)
            .title(&format!("{} v{}", appinfo.name, appinfo.version))
            .default_width(640)
            .default_height(480)
            .build();

        let grid = Grid::builder()
            .margin_top(CTRL_MARGIN)
            .margin_bottom(CTRL_MARGIN)
            .margin_start(CTRL_MARGIN)
            .margin_end(CTRL_MARGIN)
            .row_spacing(CTRL_SPACING)
            .column_spacing(CTRL_SPACING)
            .build();
        grid.set_orientation(Orientation::Horizontal);

        // token row
        let lbl_token = Label::builder().label("Token: ").width_request(40).build();
        let ent_token = Entry::builder().hexpand(true).build();
        let btn_token = Button::builder().label("Set").width_request(24).build();
        btn_token.connect_clicked(|e| {
            e.set_label("Thanks!");
        });
        grid.attach(&lbl_token, 0, 0, 1, 1);
        grid.attach(&ent_token, 1, 0, 1, 1);
        grid.attach(&btn_token, 2, 0, 1, 1);

        // group row
        let lbl_group = Label::builder().label("Group: ").width_request(40).build();
        let ent_group = Entry::builder().hexpand(true).build();
        let btn_group = Button::builder().label("Set").width_request(24).build();
        btn_group.connect_clicked(|e| {
            e.set_label("Thanks!");
        });
        grid.attach(&lbl_group, 0, 1, 1, 1);
        grid.attach(&ent_group, 1, 1, 1, 1);
        grid.attach(&btn_group, 2, 1, 1, 1);

        // thread row
        let lbl_thread = Label::builder().label("Thread: ").width_request(40).build();
        let ent_thread = Entry::builder().hexpand(true).build();
        let btn_thread = Button::builder().label("Set").width_request(24).build();
        btn_thread.connect_clicked(|e| {
            e.set_label("Thanks!");
        });
        grid.attach(&lbl_thread, 0, 2, 1, 1);
        grid.attach(&ent_thread, 1, 2, 1, 1);
        grid.attach(&btn_thread, 2, 2, 1, 1);

        // email row
        let lbl_email = Label::builder().label("Email: ").width_request(40).build();
        let ent_email = Entry::builder().hexpand(true).build();
        let btn_email = Button::builder().label("Set").width_request(24).build();
        btn_email.connect_clicked(|e| {
            e.set_label("Thanks!");
        });
        grid.attach(&lbl_email, 0, 3, 1, 1);
        grid.attach(&ent_email, 1, 3, 1, 1);
        grid.attach(&btn_email, 2, 3, 1, 1);

        window.set_child(Some(&grid));

        // last
        window.present();
    }
}
