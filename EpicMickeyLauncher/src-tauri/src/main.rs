//note 2self or whoever. macos directory system uses / and not \

/* #![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
  */
use fs_extra::dir::CopyOptions;
use futures_util::StreamExt;
use registry::Hive;
use registry::Security;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::io::Cursor;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str;
use std::sync::OnceLock;
use walkdir::WalkDir;
use registry;
extern crate dirs_next;
extern crate fs_extra;
extern crate open;
extern crate reqwest;
extern crate scan_dir;
extern crate sevenz_rust;
extern crate walkdir;
extern crate zip_extract;
use fs_extra::dir::copy;
use tauri::{Manager, Window};
#[derive(Serialize, Deserialize)]
struct ChangedFiles {
    name: String,
    modid: String,
    files: Vec<String>,
    texturefiles: Vec<String>,
    active: bool,
    update: i32,
}

#[derive(Serialize, Deserialize)]
struct ModInfo {
    name: String,
    game: String,
    description: String,
    dependencies: Vec<String>,
    custom_textures_path: String,
    custom_game_files_path: String,
    icon_path: String,
}

#[derive(Serialize, Deserialize)]
struct CheckISOResult {
    id: String,
    nkit: bool,
}

#[tauri::command]
fn open_link(url: String) {
    open::that(url).expect("Failed to open URL in default browser");
}

const CREATE_NO_WINDOW: u32 = 0x08000000;


#[tauri::command]
async fn extract_iso(
    witpath: String,
    nkit: String,
    isopath: String,
    gamename: String,
    is_nkit: bool,
    window: Window
) -> String {
    let mut extracted_iso_path = PathBuf::new();
    extracted_iso_path.push("c:/extractedwii");

    let mut source_path = PathBuf::new();
    source_path.push(&extracted_iso_path);


    /*  extracted_iso_path.push(&witpath);
     extracted_iso_path.pop();
    extracted_iso_path.push("extracted_iso");  */

    if Path::new(&extracted_iso_path).exists() {
        fs::remove_dir_all(&extracted_iso_path).expect("Failed to create temp folder");
    }

    let mut response = "".to_string();
    let mut m_isopath = isopath.clone();

    let mut remove_nkit_processed = false;
    if is_nkit {
        if nkit != "" {

            window.emit("change_iso_extract_msg", "Converting NKit to ISO...").unwrap();

            let mut proc_path = PathBuf::new();
            proc_path.push(&nkit);
            proc_path.push("ConvertToISO.exe");

            Command::new(proc_path)
                .arg(&m_isopath)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .expect("NKit failed to start");

            source_path.push("DATA");

            //HACK: probably the worst way to do this
            let p = nkit + "/Processed/Wii/";
            let paths = fs::read_dir(p).unwrap();
            let mut foundfirst = false;
            for path in paths {
                if !foundfirst {
                    let binding = path
                        .unwrap()
                        .path()
                        .to_str()
                        .expect("Can't get path")
                        .clone()
                        .to_string();

                    if binding.ends_with(".iso") {
                        m_isopath = binding;
                        foundfirst = true;
                        remove_nkit_processed = true;
                    }
                }
            }

            if !foundfirst {
                return "err_nkit".to_string();
            }
        } else {
            return "err_nkit".to_string();
        }
    }

    window.emit("change_iso_extract_msg", "Dumping ISO...").unwrap();

    Command::new(&witpath)
        .arg("extract")
        .arg(&m_isopath)
        .arg("-D")
        .arg("c:/extractedwii")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .expect("failed to execute process");

    window.emit("change_iso_extract_msg", "Cleaning Up...").unwrap();

    let mut path = dirs_next::document_dir().expect("could not get documents dir");
    path.push("Epic Mickey Launcher");
    path.push("Games");
    path.push(gamename);

    let without_data = path.clone();

    path.push("DATA");

    if !Path::new(&path).exists() {
        fs::create_dir_all(&path).expect("Couldn't create game folder");
    }

    let options = CopyOptions {
        depth: 0,
        overwrite: true,
        skip_exist: false,
        buffer_size: 64000,
        content_only: true,
        copy_inside: false,
    };

    //HACK: change this before commit. if anyone else but me is seeing this please feel free to yell several profanities at me¨

    window.emit("change_iso_extract_msg", "Injecting Game Files...").unwrap();

    if source_path.exists() {
        copy(source_path, &path, &options).expect("failed to inject game files");

        window.emit("change_iso_extract_msg", "Cleaning up....").unwrap();

        if remove_nkit_processed && Path::new(&m_isopath).exists() {
            fs::remove_file(m_isopath).expect("failed to remove converted nkit iso");
        }

        response = without_data.display().to_string();

        if Path::new(&extracted_iso_path).exists() {
            fs::remove_dir_all(extracted_iso_path).expect("Failed to remove temp folder");
        }
    } else {
        response = "err_extract".to_string();
    }

    window.emit("change_iso_extract_msg", "Finished!").unwrap();

    return response.to_string();
}

#[tauri::command]
async fn download_tool(url: String, foldername: String, window: Window) -> PathBuf {
    let mut to_pathbuf = PathBuf::new();
    to_pathbuf.push(dirs_next::document_dir().expect("could not get documents dir"));
    to_pathbuf.push("Epic Mickey Launcher");
    to_pathbuf.push(foldername);
    download_zip(url, &to_pathbuf, false, window).await;
    to_pathbuf
}

async fn download_zip(url: String, foldername: &PathBuf, local: bool, window: Window) -> String {
    fs::create_dir_all(&foldername).expect("Failed to create");

    let mut temporary_archive_path_buf = foldername.clone();

    temporary_archive_path_buf.push("temp");

    let temporary_archive_path = temporary_archive_path_buf.to_str().unwrap().to_string();

    let mut buffer;

    let mut f = File::create(&temporary_archive_path).expect("Failed to create tmpzip");

    if !local {
        let res = Client::new().get(&url).send().await.unwrap();

        let total_size = res
            .content_length()
            .ok_or(format!("Failed to get content length from '{}'", &url))
            .unwrap();

        window
            .emit(
                "download-stat",
                ModDownloadStats {
                    Download_Total: total_size.to_string(),
                    Download_Remaining: 0,
                },
            )
            .unwrap();

        buffer = reqwest::get(&url).await.unwrap().bytes_stream();

        let mut download_bytes_count = 0;

        while let Some(item) = buffer.next().await {
            let buf = item.as_ref().unwrap();

            download_bytes_count += buf.len();

            

            window
                .emit(
                    "download-stat",
                    ModDownloadStats {
                        Download_Total: total_size.to_string(),
                        Download_Remaining: download_bytes_count as i32,
                    },
                )
                .unwrap();

            f.write_all(buf).expect("Failed to write to tmpzip");
        }
    } else {
        //horrible solution
        fs::copy(&url, &temporary_archive_path).expect("Failed to copy local file");
    }

    let output = PathBuf::from(&foldername);

    let extension = extract_archive(url, temporary_archive_path, &output);

    extension
}

#[derive(Clone, serde::Serialize)]

struct ModDownloadStats {
    Download_Remaining: i32,
    Download_Total: String,
}

fn extract_archive(url: String, input_path: String, output_path: &PathBuf) -> String {
    let mut f = File::open(&input_path).expect("Couldn't open archive");
    let mut buffer = [0; 262];

    let mut archive_type = "";

    f.read(&mut buffer).expect("failed to read archive header");

    if &buffer[0..2] == "PK".as_bytes() {
        println!("Archive is Zip");

        archive_type = "zip";

        let mut f = File::open(&input_path).expect("Failed to open tmpzip");

        let mut buffer = Vec::new();

        f.read_to_end(&mut buffer).expect("Failed to read tmpzip");

        zip_extract::extract(Cursor::new(buffer), &output_path, false).expect("failed to extract");
    } else if &buffer[0..2] == "7z".as_bytes() {
        println!("Archive is 7Zip");
        archive_type = "7zip";
        sevenz_rust::decompress_file(&input_path, &output_path).expect("complete");
    } else if &buffer[257..262] == "ustar".as_bytes() {
        println!("Archive is TAR");
        archive_type = "tar";
        Command::new("tar")
            .arg("-xf")
            .arg(&input_path)
            .arg("-C")
            .arg(&output_path)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .expect("Tar failed to extract");
    } else {
        println!("Unknown archive type");
    }
    fs::remove_file(input_path).expect("Failed to remove tmpzip");
    archive_type.to_string()
}

static WINDOW: OnceLock<Window> = OnceLock::new();

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app.get_window("main").unwrap();

            _ = WINDOW.set(window);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            playgame,
            download_mod,
            change_mod_status,
            delete_mod,
            validate_mod,
            get_os,
            extract_iso,
            delete_mod_cache,
            check_iso,
            open_link,
            download_tool,
            validate_archive,
            set_dolphin_emulator_override
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
#[tauri::command]
fn get_os() -> &'static str {
    env::consts::OS
}

#[tauri::command]
fn playgame(dolphin: String, exe: String) -> i32 {
    let os = env::consts::OS;
    if Path::new(&dolphin).exists() {
        if os == "windows" {
            if dolphin.ends_with(".exe") {

                let path = find_dolphin_dir(&PathBuf::from("Config/GFX.ini"));

                Command::new(&dolphin)
                    .arg(&exe)
                    .spawn()
                    .expect("could not open exe");

                if path.exists() {
                    let mut f = File::open(&path).unwrap();

                    let mut path_buffer: String = Default::default();

                    f.read_to_string(&mut path_buffer).expect("Failed to read file");

                    path_buffer = path_buffer.replace("HiresTextures = False", "HiresTextures = True");

                    let mut new = File::create(&path).unwrap();

                    new.write_all(path_buffer.as_bytes()).expect("Failed to write to file");

                }

            } else if Path::new(&exe).exists() {
                Command::new(&dolphin)
                    .arg(&exe)
                    .spawn()
                    .expect("could not open dolphin");
            }
            return 0;
        } else {
            Command::new("open")
                .arg("-a")
                .arg(&dolphin)
                .arg(&exe)
                .spawn()
                .expect("could not open dolphin");
            return 0;
        }
    }
    return 0;
}

fn remove_first(s: &str) -> Option<&str> {
    s.chars().next().map(|c| &s[c.len_utf8()..])
}

#[tauri::command]
fn check_iso(path: String) -> CheckISOResult {
    let mut f = File::open(path).expect("Couldn't open ISO");
    let mut buffer = [0; 1000];
    f.read(&mut buffer).expect("failed to read game id");
    let id = std::str::from_utf8(&buffer[0..6]).unwrap().to_uppercase();
    let nkit = std::str::from_utf8(&buffer[0x200..0x204])
        .unwrap()
        .to_uppercase();
    let is_nkit = if nkit == "NKIT" { true } else { false };
    let res = CheckISOResult {
        id: id,
        nkit: is_nkit,
    };
    res
}

#[tauri::command]
async fn change_mod_status(
    json: String,
    dumploc: String,
    gameid: String,
    modid: String,
    platform: String,
    window: Window,
) {
    let mut data: ChangedFiles = serde_json::from_str(&json).unwrap();

    let active = data.active;

    let name = &data.name;

    if active {
        //todo: fix this shit
        download_mod(
            "".to_string(),
            name.to_string(),
            dumploc,
            gameid,
            modid,
            platform,
            window,
        )
        .await;
    } else {
        //HACK!!
        data.active = !data.active;

        let json = serde_json::to_string(&data).expect("failed to serialize");

        delete_mod(json, dumploc, gameid, platform).await;
    }

    println!("Proccess ended");
}

#[tauri::command]
async fn delete_mod(json: String, dumploc: String, gameid: String, platform: String) {
    let data: ChangedFiles = serde_json::from_str(&json).unwrap();

    let files = data.files;
    let texturefiles = data.texturefiles;

    let active = data.active;

    if active {
        let mut datafiles_path = PathBuf::new();
        datafiles_path.push(&dumploc);
        if platform == "wii" {
            datafiles_path.push("files");
        }

        let mut backup_path = PathBuf::new();
        backup_path.push(&dumploc);
        backup_path.push("backup");

        for file in files {
            let mut source_path = PathBuf::new();
            source_path.push(&backup_path);
            source_path.push(&file);

            let mut destination_path = PathBuf::new();
            destination_path.push(&datafiles_path);
            destination_path.push(&file);

            if std::path::Path::new(&source_path).exists()
                && std::path::Path::new(&destination_path).exists()
            {
                fs::copy(source_path, destination_path).unwrap();
            }
        }

        let mut p = PathBuf::from("Load/Textures/");
        p.push(&gameid);

        let dolphin_path = find_dolphin_dir(&p);

        for file in texturefiles {
            let mut path = PathBuf::new();

            path.push(&dolphin_path);

            let path_final = remove_first(&file).expect("couldn't remove slash from string");

            path.push(path_final);

            if std::path::Path::new(&path).exists() {
                fs::remove_file(&path).unwrap();
            }
        }
    }

    println!("Proccess ended");
}

#[derive(Serialize, Deserialize)]
struct ValidationInfo {
    modname: String,
    modicon: String,
    extension: String,
    validated: bool,
}

#[derive(Serialize, Deserialize)]
struct SmallArchiveValidationInfo {
    under_limit: bool,
    extension: String,
}

#[tauri::command]
fn validate_archive(path: String) -> SmallArchiveValidationInfo {
    let mut validation_info = SmallArchiveValidationInfo {
        under_limit: false,
        extension: "".to_string(),
    };
    let mut f = File::open(&path).expect("Couldn't open archive");
    let size = f.metadata().unwrap().len();

    validation_info.under_limit = size < 100000000;

    let mut buffer = [0; 262];
    f.read(&mut buffer).expect("failed to read archive header");

    if &buffer[0..2] == "PK".as_bytes() {
        println!("Archive is Zip");
        validation_info.extension = "zip".to_string();
    } else if &buffer[0..2] == "7z".as_bytes() {
        println!("Archive is 7Zip");
        validation_info.extension = "7zip".to_string();
    } else if &buffer[257..262] == "ustar".as_bytes() {
        println!("Archive is TAR");
        validation_info.extension = "tar".to_string();
    } else {
        println!("Unknown archive type");
    }
    validation_info
}

#[tauri::command]
fn delete_mod_cache(modid: String) {
    let mut path = dirs_next::config_dir().expect("could not get config dir");
    path.push(r"com.memer.eml/cachedMods");
    path.push(modid);
    if path.exists() {
        fs::remove_dir_all(path).expect("Could not remove mod cache");
    }
}

#[tauri::command]
fn set_dolphin_emulator_override(_path: String) {
    let mut path = dirs_next::config_dir().expect("could not get config dir");
    path.push(r"com.memer.eml");

    fs::create_dir_all(&path).unwrap();

    path.push("dolphinoverride");

    let mut f = File::create(&path).expect("Failed to create file");

    f.write_all(_path.as_bytes())
        .expect("Failed to write to file");
}

#[tauri::command]
async fn validate_mod(url: String, local: bool, window: Window) -> ValidationInfo {
    println!("Validating mod");

    let mut path_imgcache = dirs_next::config_dir().expect("could not get config dir");
    path_imgcache.push("cache");

    fs::create_dir_all(&mut path_imgcache).expect("Failed to create folders.");

    path_imgcache.push("temp.png");

    let mut path = dirs_next::config_dir().expect("could not get config dir");
    path.push(r"com.memer.eml/TMP");

    let mut json_path = path.clone();
    json_path.push("mod.json");

    let mut icon_path = path.clone();

    let extension = download_zip(url, &path, local, window).await;

    println!("Finished Downloading mod for validation");

    let mut validation = ValidationInfo {
        modname: "".to_string(),
        modicon: "".to_string(),
        extension: extension,
        validated: false,
    };

    if Path::exists(&json_path) {
        let json_string =
            fs::read_to_string(&json_path).expect("mod.json does not exist or could not be read");
        let json_data: ModInfo = serde_json::from_str(&json_string)
            .expect("Mod data either doesn't exist or couldn't be loaded due to formatting error.");
        icon_path.push(json_data.icon_path);

        if Path::exists(&icon_path) {
            fs::copy(icon_path, &path_imgcache).expect("Could not copy icon file to cache");
            validation.validated = true;
            validation.modicon = path_imgcache
                .to_str()
                .expect("Couldn't convert path to string.")
                .to_string();
            validation.modname = json_data.name;
        } else {
            println!("Icon file does not exist");
        }
    } else {
        println!("Mod.json does not exist");
    }
    //fs::remove_dir_all(&path).expect("Couldn't remove temporary directory");
    println!("Finished Validating mod");
    validation
}

fn correct_all_slashes(path: String) -> String {
    path.replace(r"\", "/")
}

#[tauri::command]
async fn download_mod(
    url: String,
    name: String,
    dumploc: String,
    gameid: String,
    modid: String,
    platform: String,
    window: Window,
) -> String {
    let mut path = dirs_next::config_dir().expect("could not get config dir");
    path.push(r"com.memer.eml/cachedMods");

    let mut full_path = path.clone();
    full_path.push(&modid);

    let os = env::consts::OS;

    // download

    let mut mod_json_path_check = full_path.clone();
    mod_json_path_check.push("mod.json");

    if !mod_json_path_check.exists() && !url.is_empty() {
        fs::create_dir_all(&full_path).expect("Couldn't create mod cache folder");

        let is_local = !url.starts_with("http");

        download_zip(url, &full_path, is_local, window).await;
        println!("done downloading");
    }

    let mut path_json = full_path.clone();
    path_json.push("mod.json");

    let json_string =
        fs::read_to_string(path_json).expect("mod.json does not exist or could not be read");

    let json_data: ModInfo = serde_json::from_str(&json_string)
        .expect("Mod data either doesn't exist or couldn't be loaded due to formatting error.");

    //inject files

    let mut path_textures = full_path.clone();
    let mut path_datafiles = full_path.clone();

    path_textures.push(&json_data.custom_textures_path);
    path_datafiles.push(&json_data.custom_game_files_path);

    let mut files_to_restore: Vec<String> = Vec::new();

    //inject DATA files into current dump
    if Path::new(&path_datafiles).exists() {
        let mut path_final_location = PathBuf::new();

        let dumploc_clone = dumploc.clone();

        path_final_location.push(&dumploc);

        if platform == "wii" {
            path_final_location.push("files");
        }

        //backup files
        let mut path_backup = PathBuf::new();

        path_backup.push(dumploc_clone);

        path_backup.push("backup");

        fs::create_dir_all(&path_backup).expect("couldn't create backup folder");

        let path_search_clone = path_datafiles.clone();

        let path_datafiles_clone_str = path_datafiles.clone();

        let path_datafiles_str = correct_all_slashes(
            path_datafiles_clone_str
                .into_os_string()
                .into_string()
                .unwrap(),
        );

        let mut dirs: Vec<String> = Vec::new();

        //we're copying the folders too since you never know if the mod put in an extra

        for entry in WalkDir::new(&path_search_clone) {
            let p = entry.unwrap();

            if !p.path().is_file() {
                let p_str = correct_all_slashes(
                    p.path()
                        .to_str()
                        .expect("Couldn't convert path to string.")
                        .to_string(),
                );

                //HACK: this can probably be done better right?
                let dont_end_with = format!(r"{}{}", "/", json_data.custom_game_files_path);

                if p_str.ends_with(&dont_end_with) {
                    continue;
                }

                let p_str_shortened = p_str.replace(&path_datafiles_str, "");

                let p_str_final =
                    remove_first(&p_str_shortened).expect("couldn't remove slash from string");

                dirs.push(p_str_final.to_string());
            }
        }

        for directory in &dirs {
            let mut dir = PathBuf::new();
            dir.push(&path_backup);
            dir.push(directory);

            fs::create_dir_all(&dir).expect("Failed to create folders.");
        }

        println!("Created Folders");

        let mut files: Vec<String> = Vec::new();

        //backup files

        for entry in WalkDir::new(&path_search_clone) {
            let p = entry.unwrap();

            if p.path().is_file() {
                let p_str = correct_all_slashes(
                    p.path()
                        .to_str()
                        .expect("Couldn't convert path to string.")
                        .to_string(),
                );

                let p_str_shortened = &p_str.replace(&path_datafiles_str, "");

                //get rid of slash

                let p_str_final =
                    remove_first(&p_str_shortened).expect("couldn't remove slash from string");

                files.push(p_str_final.to_string());
            }
        }

        for file in &files {
            let mut source = PathBuf::new();
            source.push(&dumploc);
            if platform == "wii" {
                source.push("files");
            }
            source.push(file);

            let mut destination = PathBuf::new();
            destination.push(&path_backup);
            destination.push(file);

            if std::path::Path::new(&source).exists()
                && !std::path::Path::new(&destination).exists()
            {
                fs::copy(&source, destination).expect("couldn't copy file to backup");
            }

            files_to_restore.push(file.to_string());
        }

        println!("Created Files");

        // copy modded files to the game

        let options = CopyOptions {
            depth: 0,
            overwrite: true,
            skip_exist: false,
            buffer_size: 128000,
            content_only: true,
            copy_inside: false,
        };

        copy(&path_datafiles, path_final_location, &options).expect("failed to inject mod files");
    }

    let mut texturefiles: Vec<String> = Vec::new();

    let mut p = PathBuf::from("Load/Textures/");
    p.push(&gameid);

    let dolphin_path = find_dolphin_dir(&p);

    fs::create_dir_all(&dolphin_path).expect("Failed to create dolphin folder.");

    //inject texture files into dolphin config
    if Path::new(&path_textures).exists() {
        let path_textures_str = &path_textures
            .clone()
            .into_os_string()
            .into_string()
            .unwrap();

        for entry in WalkDir::new(&path_textures) {
            let p = entry.unwrap();

            if p.path().is_file() {
                let p_str = p.path().to_str().expect("Couldn't convert path to string.");
                let p_str_final = &p_str.replace(path_textures_str, "");
                texturefiles.push(p_str_final.to_string());
            }
        }

        let options = CopyOptions {
            depth: 0,
            overwrite: true,
            skip_exist: false,
            buffer_size: 128000,
            content_only: true,
            copy_inside: false,
        };

        fs::create_dir_all(&path).expect("Failed to create folders.");

        copy(&path_textures, &dolphin_path, &options).expect("failed to inject texture files");
    }

    let changed_files_json = ChangedFiles {
        name: name,
        files: files_to_restore,
        texturefiles: texturefiles,
        modid: modid,
        active: true,
        update: 0,
    };

    let json = serde_json::to_string(&changed_files_json).unwrap();

    println!("Process ended successfully"); 
    json.into()
}

fn find_dolphin_dir(where_in: &PathBuf) -> PathBuf {
    let os = env::consts::OS;

    let mut dolphin_path = PathBuf::new();

    let mut path = dirs_next::config_dir().expect("could not get config dir");
    path.push(r"com.memer.eml");
    path.push("dolphinoverride");

    if !path.exists() {
        if os == "macos" {
            dolphin_path = dirs_next::config_dir().expect("Failed to get config path");
            dolphin_path.push(Path::new(r"Dolphin"));
            dolphin_path.push(where_in);
        } else {
            let regkey = Hive::CurrentUser.open(r"Software\Dolphin Emulator", Security::Read).unwrap();
            let path = regkey.value("UserConfigPath").unwrap().to_string();

            dolphin_path.push(path);
            dolphin_path.push(where_in);

            print!("{}", dolphin_path.display());
        }
    } else {
        let mut f = File::open(path).unwrap();
        dolphin_path.clear();
        let mut buff = String::new();
        f.read_to_string(&mut buff).expect("Failed to read file");
        dolphin_path.push(buff);
        dolphin_path.push(where_in);
    }

    dolphin_path
}
