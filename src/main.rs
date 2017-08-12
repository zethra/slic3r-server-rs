extern crate iron;
#[macro_use]
extern crate router;
extern crate mount;
extern crate staticfile;
extern crate tempdir;
extern crate multipart;

use iron::prelude::*;
use iron::status;
use iron::Request;
use router::Router;
use staticfile::Static;
use mount::Mount;
use tempdir::TempDir;
use multipart::server::{Multipart, Entries, SaveResult, SavedFile};

use std::path::Path;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::process::*;

fn process_multipart(req: &mut Request) -> IronResult<Response> {
    match Multipart::from_request(req) {
        Ok(mut multipart) => {
            match multipart.save().temp() {
                SaveResult::Full(entries) => slice_file(entries),
                SaveResult::Partial(entries, reason) => {
                    slice_file(entries.keep_partial())?;
                    Err(IronError::new(reason.unwrap_err(), status::InternalServerError))
                }
                SaveResult::Error(error) => Err(IronError::new(error, status::InternalServerError)),
            }
        }
        Err(err) => {
            println!("{:?}", err);
            Ok(Response::with((status::BadRequest, "The request is not multipart")))
        }
    }
}

fn slice_file(entries: Entries) -> IronResult<Response> {
    let config_path = Path::new("./config");
    let files = match entries.files.get("stl") {
        Some(files) => files,
        None => {
            return Ok(Response::with((status::BadRequest, "Must include file under field \"stl\"")))
        }
    };
    if files.len() != 1 {
        return Ok(Response::with((status::BadRequest, "Only one file can be sliced per request")));
    }
    let file = &files[0];

    let cwd = match TempDir::new("slicer-server") {
        Ok(cwd) => cwd,
        Err(error) => {
            return Err(IronError::new(error, status::InternalServerError));
        }
    };
    let input_path = cwd.path().join("input.stl");
    let output_path = cwd.path().join("output.gcode");
    if let Err(error) = fs::rename(&file.path, &input_path) {
        return Err(IronError::new(error,
                                  (status::InternalServerError, "Couldn't copy stl file")));
    }
    let mut cmd = Command::new("slic3r");
    cmd.arg(input_path)
        .arg("--output")
        .arg(&output_path);
    if let Some(config) = entries.fields.get("config") {
        let conf_file_path = config_path.join(config.trim());
        println!("{:?}", conf_file_path);
        if !conf_file_path.exists() {
            return Ok(Response::with((status::BadRequest, "Config file doesn't exist")));
        }
        cmd.arg("--load").arg(conf_file_path.as_os_str());
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Err(IronError::new(error,
                                      (status::InternalServerError, "Slic3r failed to run")));
        }
    };
    let exit_code = match child.wait() {
        Ok(exit_code) => exit_code,
        Err(error) => {
            return Err(IronError::new(error,
                                      (status::InternalServerError, "Slic3r failed to finish")));
        }
    };
    if !exit_code.success() {
        return Ok(Response::with((status::InternalServerError, "Slic3r didn't return success")));
    }

    let mut file = match File::open(&output_path) {
        Ok(file) => file,
        Err(error) => {
            return Err(IronError::new(error,
                                      (status::InternalServerError, "Couldn't open gocde file")));
        }
    };

    let mut contents = String::new();
    if let Err(error) = file.read_to_string(&mut contents) {
        return Err(IronError::new(error,
                                  (status::InternalServerError, "Couldn't read gcode file")));
    }

    Ok(Response::with((status::Ok, contents)))
}

fn main() {
    let config_path = Path::new("./config");
    if !config_path.exists() {
        fs::create_dir(config_path).unwrap();
    }

    // let mut router = Router::new();


    let mut mount = Mount::new();
    mount.mount("/", process_multipart);
    Iron::new(mount).http("127.0.0.1:7766").unwrap();
}