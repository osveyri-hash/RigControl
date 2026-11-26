//! Что работает в фоновой службе, пока окно закрыто: подсветка клавиатуры (KbLight), ARGB и экран помпы
//! (Service), вентиляторы платы (FanControl) и заряд клавиатуры (BatteryWatch) — по settings.json.

use std::time::Duration;

use crate::battery::BatteryWatch;
use crate::fans::FanControl;
use crate::kb_light::KbLight;
use crate::service::Service;
use crate::settings::Settings;

pub struct Devices {
    svc: Option<Service>,
    kb: Option<KbLight>,
    fans: Option<FanControl>,
    battery: Option<BatteryWatch>,
}

impl Devices {
    /// Прочитать настройки и запустить всё.
    pub fn start() -> Self {
        let s = Settings::load();
        crate::light_mirror::set_rates(s.effect_rates.as_ref());
        crate::lhm::reset();
        Self {
            svc: Some(Service::start_with(&s, false, s.auto_output)),
            kb: Some(KbLight::start(&s)),
            fans: Some(FanControl::start(&s)),
            battery: Some(BatteryWatch::start(None)),
        }
    }

    /// Заряд клавиатуры по радио (для подсказки значка в трее).
    pub fn battery(&self) -> Option<u8> {
        self.battery.as_ref().and_then(|b| b.state().level)
    }

    /// Отдать устройства: всё остановить, вентиляторы — плате, помощника датчиков закрыть.
    pub fn stop(mut self) {
        if let Some(s) = self.svc.take() {
            s.stop();
        }
        drop(self.kb.take());
        if let Some(f) = self.fans.take() {
            f.shutdown();
        }
        drop(self.battery.take());
        // Поток вентиляторов отдаёт каналы плате в течение секунды после остановки.
        std::thread::sleep(Duration::from_millis(1300));
        crate::lhm::shutdown();
    }
}
