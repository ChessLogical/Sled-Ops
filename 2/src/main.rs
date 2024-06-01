use actix_files as fs;
use actix_multipart::Multipart;
use actix_web::{web, App, Error, HttpResponse, HttpServer, Responder};
use futures_util::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::io::Write;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct Post {
    id: String,
    title: String,
    message: String,
    file: Option<String>,
}

async fn save_post(
    db: web::Data<Db>,
    upload_dir: web::Data<String>,
    mut payload: Multipart,
) -> Result<HttpResponse, Error> {
    let mut title = String::new();
    let mut message = String::new();
    let mut filename: Option<String> = None;

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
            "file" => {
                if let Some(filename_value) = content_disposition.get_filename() {
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
            _ => (),
        }
    }

    let post = Post {
        id: Uuid::new_v4().to_string(),
        title,
        message,
        file: filename.clone(),
    };

    let serialized = serde_json::to_vec(&post).unwrap();
    db.insert(&post.id, serialized).unwrap();
    db.flush().unwrap();

    Ok(HttpResponse::SeeOther()
        .append_header(("Location", "/"))
        .finish())
}

async fn index(db: web::Data<Db>) -> impl Responder {
    let mut posts = Vec::new();
    for item in db.iter().values() {
        let post: Post = serde_json::from_slice(&item.unwrap()).unwrap();
        posts.push(post);
    }

    posts.sort_by(|a, b| b.id.cmp(&a.id));

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
                </div>"#,
                post.title,
                post.message,
                file_html
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
                <input type="file" name="file" accept=".jpg,.gif,.png,.mp3,.mp4,.webm,.webp" required><br>
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
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
