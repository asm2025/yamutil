use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};
use relm::factory::Position;
use relm::prelude::*;
use rustmix::*;
use std::sync::Arc;

use crate::{error::ExitCodeError, service::*};

const APP_ID: &str = "com.github.asm.yamutil";

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
        application.connect_activate(move |app| {
            Self::build_ui(app, &appinfo);
        });

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
        window.present();
    }
}
