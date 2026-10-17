//! `SocialBO`: reports against drivers.

use crate::dao::report::{self, ReportEntity};
use crate::{Fault, World};

// port: bo/SocialBO#sendReport(Long,Long,Integer,String,Integer,Integer,Long)
#[allow(clippy::too_many_arguments)]
pub fn send_report(
    w: &World,
    persona_id: i64,
    abuser_persona_id: i64,
    petition_type: i32,
    description: String,
    custom_car_id: i32,
    chat_minutes: i32,
    hacks_detected: i64,
) -> Result<(), Fault> {
    let mut report_entity = ReportEntity {
        abuser_persona_id: Some(abuser_persona_id),
        chat_minutes: Some(chat_minutes),
        custom_car_id: Some(custom_car_id),
        description: Some(description),
        persona_id: Some(persona_id),
        petition_type: Some(petition_type),
        hacksdetected: Some(hacks_detected),
        ..Default::default()
    };
    report::insert(w.db(), &mut report_entity)?;
    Ok(())
}
