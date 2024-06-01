use actix_files as fs;
use actix_multipart::Multipart;
use actix_web::{web, App, Error, HttpResponse, HttpServer, Responder};
use futures_util::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::time::SystemTime;
use std::io::Write;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone)]
struct Post {
    id: String,
    parent_id: Option<String>,
    title: String,
    message: String,
    file: Option<String>,
    #[serde(default = "default_timestamp")]
    timestamp: u64,
}

fn default_timestamp() -> u64 {
    0
}

async fn save_post(
    db: web::Data<Db>,
    upload_dir: web::Data<String>,
    mut payload: Multipart,
) -> Result<HttpResponse, Error> {
    let mut title = String::new();
    let mut message = String::new();
    let mut filename: Option<String> = None;
    let mut parent_id: Option<String> = None;

    // Get the current timestamp
    let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();

    // Process each field in the multipart payload
    while let Ok(Some(mut field)) = payload.try_next().await {
        let content_disposition = field.content_disposition();
        let field_name = content_disposition.get_name().unwrap().to_string();

        match field_name.as_str() {
            "title" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk.unwrap();
                    title.push_str(std::str::from_utf8(&data).unwrap());
                }
            }
            "message" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk.unwrap();
                    message.push_str(std::str::from_utf8(&data).unwrap());
                }
            }
            "parent_id" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk.unwrap();
                    parent_id = Some(std::str::from_utf8(&data).unwrap().to_string());
                }
            }
            "file" => {
                if let Some(filename_value) = content_disposition.get_filename() {
                    if !filename_value.is_empty() {
                        let file_extension = filename_value
                            .split('.')
                            .last()
                            .map(String::from)
                            .unwrap_or_else(|| "tmp".to_string());
                        let file_name = format!("{}.{}", Uuid::new_v4(), file_extension);
                        let filepath = format!("{}/{}", upload_dir.get_ref(), &file_name);

                        let mut f = web::block(|| std::fs::File::create(filepath)).await??;

                        while let Some(chunk) = field.next().await {
                            let data = chunk.unwrap();
                            f = web::block(move || {
                                f.write_all(&data).map(|_| f)
                            }).await??;
                        }

                        filename = Some(file_name);
                    }
                }
            }
            _ => (),
        }
    }

    let post = Post {
        id: Uuid::new_v4().to_string(),
        parent_id,
        title,
        message,
        file: filename.clone(),
        timestamp,
    };

    let serialized = serde_json::to_vec(&post).unwrap();
    db.insert(&post.id, serialized).unwrap();
    db.flush().unwrap();

    if let Some(parent_id) = post.parent_id {
        Ok(HttpResponse::SeeOther()
            .append_header(("Location", format!("/post/{}", parent_id)))
            .finish())
    } else {
        Ok(HttpResponse::SeeOther()
            .append_header(("Location", "/"))
            .finish())
    }
}

async fn view_post(db: web::Data<Db>, post_id: web::Path<String>) -> impl Responder {
    let mut post = None;
    let mut replies = Vec::new();

    for item in db.iter().values() {
        let current_post: Post = serde_json::from_slice(&item.unwrap()).unwrap_or_else(|_| Post {
            id: String::new(),
            parent_id: None,
            title: String::new(),
            message: String::new(),
            file: None,
            timestamp: 0,
        });

        if current_post.id == *post_id {
            post = Some(current_post.clone());
        } else if let Some(parent_id) = &current_post.parent_id {
            if parent_id == &*post_id {
                replies.push(current_post.clone());
            }
        }
    }

    // Sort replies by timestamp in descending order
    replies.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    replies.reverse();

    if let Some(post) = post {
        let file_html = if let Some(file) = &post.file {
            let extension = file.split('.').last().unwrap_or("");
            match extension {
                "jpg" | "jpeg" | "png" | "gif" | "webp" => format!(r#"<img src="/static/uploads/{}" width="200" height="200" alt="Image">"#, file),
                "mp4" | "webm" => format!(r#"<video width="200" height="200" controls><source src="/static/uploads/{}" type="video/{}">Your browser does not support the video tag.</video>"#, file, extension),
                "mp3" => format!(r#"<audio controls><source src="/static/uploads/{}" type="audio/mpeg">Your browser does not support the audio element.</audio>"#, file),
                _ => format!(r#"<a href="/static/uploads/{}">Download file</a>"#, file),
            }
        } else {
            String::new()
        };

        let replies_html = replies
            .iter()
            .enumerate()
            .map(|(index, reply)| {
                let reply_file_html = if let Some(file) = &reply.file {
                    let extension = file.split('.').last().unwrap_or("");
                    match extension {
                        "jpg" | "jpeg" | "png" | "gif" | "webp" => format!(r#"<img src="/static/uploads/{}" width="200" height="200" alt="Image">"#, file),
                        "mp4" | "webm" => format!(r#"<video width="200" height="200" controls><source src="/static/uploads/{}" type="video/{}">Your browser does not support the video tag.</video>"#, file, extension),
                        "mp3" => format!(r#"<audio controls><source src="/static/uploads/{}" type="audio/mpeg">Your browser does not support the audio element.</audio>"#, file),
                        _ => format!(r#"<a href="/static/uploads/{}">Download file</a>"#, file),
                    }
                } else {
                    String::new()
                };

                format!(
                    r#"<div>
                        <h4>Reply {}</h4>
                        <p>{}</p>
                        {}
                        <hr>
                    </div>"#,
                    index + 1,
                    reply.message,
                    reply_file_html
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let html = format!(
            r#"<!DOCTYPE html>
            <html lang="en">
            <head>
                <meta charset="UTF-8">
                <title>View Post</title>
            </head>
            <body>
                <a href="/">Back to Main Board</a>
                <form action="/submit" method="post" enctype="multipart/form-data">
                    <input type="hidden" name="parent_id" value="{}">
                    <input type="text" name="title" placeholder="Title" maxlength="15" required><br>
                    <textarea name="message" placeholder="Message" maxlength="100000" required></textarea><br>
                    <input type="file" name="file" accept=".jpg,.gif,.png,.mp3,.mp4,.webm,.webp"><br>
                    <button type="submit">Submit</button>
                </form>
                <hr>
                <div>
                    <h4>Original Post</h4>
                    <h3>{}</h3>
                    <p>{}</p>
                    {}
                </div>
                <hr>
                {}
            </body>
            </html>"#,
            post.id,
            post.title,
            post.message,
            file_html,
            replies_html
        );

        HttpResponse::Ok().content_type("text/html").body(html)
    } else {
        HttpResponse::NotFound().finish()
    }
}

async fn index(db: web::Data<Db>) -> impl Responder {
    let mut posts = Vec::new();
    for item in db.iter().values() {
        let post: Post = serde_json::from_slice(&item.unwrap()).unwrap_or_else(|_| Post {
            id: String::new(),
            parent_id: None,
            title: String::new(),
            message: String::new(),
            file: None,
            timestamp: 0,
        });
        if post.parent_id.is_none() {
            posts.push(post);
        }
    }

    // Sort posts by timestamp in descending order
    posts.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let posts_html = posts
        .iter()
        .map(|post| {
            let file_html = if let Some(file) = &post.file {
                let extension = file.split('.').last().unwrap_or("");
                match extension {
                    "jpg" | "jpeg" | "png" | "gif" | "webp" => format!(r#"<img src="/static/uploads/{}" width="200" height="200" alt="Image">"#, file),
                    "mp4" | "webm" => format!(r#"<video width="200" height="200" controls><source src="/static/uploads/{}" type="video/{}">Your browser does not support the video tag.</video>"#, file, extension),
                    "mp3" => format!(r#"<audio controls><source src="/static/uploads/{}" type="audio/mpeg">Your browser does not support the audio element.</audio>"#, file),
                    _ => format!(r#"<a href="/static/uploads/{}">Download file</a>"#, file),
                }
            } else {
                String::new()
            };

            format!(
                r#"<div>
                    <h3>{}</h3>
                    <p>{}</p>
                    {}
                    <a href="/post/{}">Reply</a>
                    <hr>
                </div>"#,
                post.title,
                post.message,
                file_html,
                post.id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <title>Post Form</title>
        </head>
        <body>
            <form action="/submit" method="post" enctype="multipart/form-data">
                <input type="text" name="title" placeholder="Title" maxlength="15" required><br>
                <textarea name="message" placeholder="Message" maxlength="100000" required></textarea><br>
                <input type="file" name="file" accept=".jpg,.gif,.png,.mp3,.mp4,.webm,.webp"><br>
                <button type="submit">Submit</button>
            </form>
            <hr>
            {}
        </body>
        </html>"#,
        posts_html
    );

    HttpResponse::Ok().content_type("text/html").body(html)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let db = sled::open("my_db").unwrap();
    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./static/uploads".to_string());
    std::fs::create_dir_all(&upload_dir).unwrap();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db.clone()))
            .app_data(web::Data::new(upload_dir.clone()))
            .service(fs::Files::new("/static", "./static").show_files_listing())
            .route("/", web::get().to(index))
            .route("/submit", web::post().to(save_post))
            .route("/post/{id}", web::get().to(view_post))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
