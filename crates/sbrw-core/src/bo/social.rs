//! `SocialBO`: reports against drivers.

use crate::dao::report::{self, ReportEntity};
use crate::{Fault, World};

// port: bo/SocialBO#sendReport(Long,Long,Integer,String,Integer,Integer,Long)
/// Any of a player's report's fields may be missing.
#[allow(clippy::too_many_arguments)]
pub fn send_report(
    w: &World,
    persona_id: Option<i64>,
    abuser_persona_id: Option<i64>,
    petition_type: Option<i32>,
    description: Option<String>,
    custom_car_id: Option<i32>,
    chat_minutes: Option<i32>,
    hacks_detected: i64,
) -> Result<(), Fault> {
    let mut report_entity = ReportEntity {
        abuser_persona_id,
        chat_minutes,
        custom_car_id,
        description,
        persona_id,
        petition_type,
        hacksdetected: Some(hacks_detected),
        ..Default::default()
    };
    report::insert(w.db(), &mut report_entity)?;
    Ok(())
}
