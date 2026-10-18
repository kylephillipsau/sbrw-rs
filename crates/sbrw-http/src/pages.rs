//! The original's web pages (`src/main/webapp`): a front page, and forms for
//! invite tickets, password recovery, reloads and announcements, which post
//! to the endpoints of the same names. Each JSP echoed one request parameter
//! (the token, or the reset key) into a hidden field as is; here it is
//! HTML-escaped, so a crafted link can't inject markup.

use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;

struct Page {
    name: &'static str,
    /// The request parameter the page echoes, if any.
    parameter: Option<&'static str>,
    html: &'static str,
}

const PAGES: &[Page] = &[
    Page {
        name: "index.jsp",
        parameter: None,
        html: r#"<h1>Soapbox Race World server</h1>
<p>There is nothing to see here.</p>
"#,
    },
    Page {
        name: "ticket.jsp",
        parameter: Some("ticketAuth"),
        html: r#"<html>
<head></head>
<body>
	<br />
	<br />

	<form action="Engine.svc/CreateTicket" method="post">
		Discord Name: <input type="text" name="discordName" /><br /> <br />
		<input type="hidden" name="ticketAuth"
			value="%VALUE%" /> <br /> <input
			type="submit" value="create ticket" />
	</form>
	<br />
	<br />
	<br />
	<br />
</body>
</html>
"#,
    },
    Page {
        name: "forgotPasswd.jsp",
        parameter: None,
        html: r#"<html>
<head></head>
<body>
	<form action="Engine.svc/RecoveryPassword/forgotPassword" method="post">
		Email: <input type="text" name="email" /><br /> <input type="submit"
			value="Reset my password" />
	</form>
</body>
</html>
"#,
    },
    Page {
        name: "password.jsp",
        parameter: Some("randomKey"),
        html: r#"<html>
<head></head>
<body>
	<form action="Engine.svc/RecoveryPassword/resetPassword" method="post">
		New password: <input type="password" name="password" /><br />
		Confirm new password: <input type="password" name="passwordconf" /><br />
		<input type="hidden" name="randomKey" value="%VALUE%" />
		<input type="submit" value="Change password" />
	</form>
</body>
</html>
"#,
    },
    Page {
        name: "reload-parameters.jsp",
        parameter: Some("adminAuth"),
        html: r#"<html>
<head>
    <title>Reload Parameters</title>
</head>
<body>
<form action="Engine.svc/ReloadParameters" method="post">
    <input type="hidden" name="adminAuth"
           value="%VALUE%"/> <br/> <input
        type="submit" value="Reload parameters"/>
</form>
</body>
</html>
"#,
    },
    Page {
        name: "reload-login-announcements.jsp",
        parameter: Some("adminAuth"),
        html: r#"<html>
<head>
    <title>Reload Login Announcements</title>
</head>
<body>
<p><strong>Note:</strong> This action clears the login announcement cache. It does not immediately display new
    announcements to players already in-game.</p>
<form action="Engine.svc/ReloadLoginAnnouncements" method="post">
    <input type="hidden" name="adminAuth"
           value="%VALUE%"/> <br/> <input
        type="submit" value="Reload login announcements"/>
</form>
</body>
</html>
"#,
    },
    Page {
        name: "send-announcement.jsp",
        parameter: Some("announcementAuth"),
        html: r#"<html>
<head>
    <title>Send Chat Announcement</title>
</head>
<body>
<form action="Engine.svc/SendAnnouncement" method="post">
    Message: <textarea name="message"></textarea>
    <br/> <br/>
    <input type="hidden" name="announcementAuth"
           value="%VALUE%"/> <br/> <input
        type="submit" value="send announcement"/>
</form>
</body>
</html>
"#,
    },
];

/// `/`: the welcome file.
pub async fn index() -> Response {
    render(&PAGES[0], &HashMap::new())
}

/// `/<name>.jsp`.
pub async fn page(Path(name): Path<String>, Query(query): Query<HashMap<String, String>>) -> Response {
    match PAGES.iter().find(|p| p.name == name) {
        Some(page) => render(page, &query),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn render(page: &Page, query: &HashMap<String, String>) -> Response {
    let html = match page.parameter {
        // `<%= request.getParameter(..) %>` prints a missing parameter as "null".
        Some(name) => page.html.replace("%VALUE%", &escape(query.get(name).map_or("null", String::as_str))),
        None => page.html.to_owned(),
    };
    ([(CONTENT_TYPE, "text/html;charset=UTF-8")], html).into_response()
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}
