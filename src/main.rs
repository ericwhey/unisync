use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::LinkedList;
use std::env::args;
use std::ffi::c_int;
use std::fs;
use std::fs::File;
use std::io::{Write, BufReader, BufRead};
use std::path::{Path, self, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{SystemTime, Duration, UNIX_EPOCH};
use chrono::{Utc, NaiveDateTime, Local};
use chrono::prelude::DateTime;
use std::os::unix::fs::PermissionsExt;

use sha256::try_digest;
use walkdir::{WalkDir, DirEntry};

pub struct Entry {
    pub status: String,
    pub timestamp: u64,
    pub size: u64,
    pub perms: u32,
    pub hash: String,
    pub path: String
}

impl Entry {
    pub fn clone(&self) -> Self {
        return Entry {
            status: String::from(self.status.to_string()),
            timestamp: self.timestamp.clone(),
            size: self.size.clone(),
            perms: self.perms.clone(),
            hash: String::from(self.hash.to_string()),
            path: String::from(self.path.to_string())
        }
    }
}

impl Entry {
    pub fn new(line: String) -> Self {
        let mut split = line.split("\t");
        return Entry {
            status: String::from(split.next().unwrap_or("NEW")),
            timestamp: split.next().expect("Problem").parse::<u64>().unwrap(),
            size: split.next().expect("Problem").parse::<u64>().unwrap(),
            perms: split.next().expect("Problem").parse::<u32>().unwrap(),
            hash: String::from(split.next().unwrap_or("")),
            path: String::from(split.next().expect("No path").trim())
        }
    }
}

impl Entry {
    pub fn from_dir_entry(dir_entry: DirEntry, root: String) -> Self {
        //println!("Root is {}",root);
        //println!("Path is {}", dir_entry.path().display());
        let path = dir_entry.path().strip_prefix(root).unwrap().to_str().unwrap();
        let timestamp;
        let size;
        let perms;
        if let Ok(metadata) = dir_entry.metadata() {
            let a = metadata.modified().expect("Should be a modified time");
            timestamp = a.duration_since(SystemTime::UNIX_EPOCH)
              .expect("File A thinks it was created before Epoch")
              .as_secs();
            size = metadata.len();
            perms = metadata.permissions().mode();

            return Entry {
                status: String::from("NEW"),
                timestamp: timestamp,
                size: size,
                perms: perms,
                hash: String::from(""),
                path: String::from(path.trim())
            };
        } else {
            return Entry{
                status: String::from("ERROR"),
                timestamp: 0,
                size: 0,
                perms: 0,
                hash: String::from(""),
                path: String::from(path.trim())
            }
        }

    }
}

impl Entry {
    pub fn hash_path(&mut self, path: &Path) {
        self.hash = try_digest(path).unwrap();
    }
}

impl Entry {
    pub fn to_string(&self) -> String {
        return String::from(format!("{}\t{}\t{}\t{}\t{}\t{}", self.status, self.timestamp, self.size, self.perms, self.hash, self.path));
    }
}

fn scan(root: String, tx:Sender<Entry>) {
    //let mut list = LinkedList::new();
    //let root = ".".to_owned();
    let root_full = root.to_owned() + "/";
    let last_path = root_full.to_owned() + ".unisync/last.txt";
    let next_path: String = root_full.to_owned() + ".unisync/next.txt";

    fs::create_dir_all(root_full.to_owned() + ".unisync").unwrap();

    let mut reader:BufReader<File>;
    if Path::new(last_path.as_str()).exists() {
        let mut file_done = false;
        let input = File::open(last_path.to_owned()).unwrap();
        reader = BufReader::new(input);

        let mut line = String::new();
        reader.read_line(&mut line).expect("Should work");
        let mut last_entry = Entry::new(line);
        //println!("Last line is {}",last_entry.to_string());

        let mut output = File::create(next_path.to_owned()).unwrap();
        let mut next_entry;
        for dir_entry in WalkDir::new(root.to_owned())
                .sort_by_key(|a| a.file_name().to_owned())
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| !e.path().starts_with(root_full.to_owned() + ".unisync/"))
                .filter(|e| !e.file_type().is_dir()) {

            //let path = dir_entry.path();
    
            next_entry = Entry::from_dir_entry(dir_entry.to_owned(), root_full.to_owned());
            let mut compare = last_entry.path.cmp(&next_entry.path);
            
            while compare == Ordering::Less && !file_done {
                //println!("Got LESS {} {}",last_entry.path,next_entry.path);
                last_entry.status = String::from("DELETED");
                writeln!(output,"{}",last_entry.to_string());
                tx.send(last_entry.clone());
                let mut line = String::new();
                let bytes = reader.read_line(&mut line).unwrap();
                if bytes == 0 {
                    file_done = true;
                } else {
                    last_entry = Entry::new(line);
                    compare = last_entry.path.cmp(&next_entry.path);
                }
            }
            if compare == Ordering::Greater {
                println!("");
                println!("Got GREATER {} {}",last_entry.path,next_entry.path);
                let path = dir_entry.path();
                next_entry.hash_path(path);
                writeln!(output,"{}",next_entry.to_string());
                tx.send(next_entry.clone());
            } else if compare == Ordering::Equal {
                if next_entry.timestamp == last_entry.timestamp && next_entry.size == last_entry.size {
                    next_entry.hash = String::from(last_entry.hash.as_str());
                } else {
                    next_entry.status = String::from("MODIFIED");
                    next_entry.hash_path(dir_entry.path());
                }

                writeln!(output,"{}",next_entry.to_string());
                tx.send(next_entry.clone());
                let mut line = String::new();
                let bytes = reader.read_line(&mut line).unwrap();
                if bytes == 0 {
                    file_done = true;
                } else {
                    last_entry = Entry::new(line);
                }
            } else if file_done {
                next_entry.hash_path(dir_entry.path());
                writeln!(output,"{}",next_entry.to_string());
                tx.send(next_entry.clone());
            }
        }
        while !file_done {
            last_entry.status = String::from("DELETED");
            writeln!(output,"{}",last_entry.to_string());
            tx.send(last_entry.clone());
            let mut line = String::new();
            let bytes = reader.read_line(&mut line).unwrap();
            if bytes != 0 {
                last_entry = Entry::new(line);
            } else {
                file_done = true;
            } 
        }

        fs::rename(next_path, last_path);

    } else {
        let mut output = File::create(next_path.to_owned()).unwrap();

        for entry in WalkDir::new(root)
                .sort_by_key(|a| a.file_name().to_owned())
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| !e.path().starts_with(root_full.to_owned() + ".unisync/"))
                .filter(|e| !e.file_type().is_dir()) {
    
            let mut next_entry = Entry::from_dir_entry(entry.to_owned(), root_full.to_owned());
            next_entry.hash_path(entry.path());
            //println!("Next line is {}", new_entry.to_string());
            writeln!(output,"{}",next_entry.to_string());
            tx.send(next_entry.clone());
        }

        fs::rename(next_path, last_path);
    }

}

fn main() {
    let args: Vec<String> = args().collect();

    let mut root1 = String::from(&args[1]);
    let mut root2 = String::from(&args[2]);

    let root1c = root1.clone();
    let root2c = root2.clone();

    let (tx1, rx1) = mpsc::channel();
    let (tx2, rx2) = mpsc::channel();

    thread::spawn(move || {
        scan(root1c,tx1);
    });

    thread::spawn(move || {
        scan(root2c, tx2);
    });
    

    /*for entry in list1.iter_mut() {
        println!("{}",entry.to_string());
    }
    for entry in list2.iter_mut() {
        println!("{}",entry.to_string());
    }*/

    let mut iter1 = rx1.iter();
    let mut iter2 = rx2.iter();

    let mut entry1 = iter1.next();
    let mut entry2 = iter2.next();

    println!("Starting");

    while let (Some(entry1u),Some(entry2u)) = (&entry1, &entry2) {
        let compare = entry1u.path.cmp(&entry2u.path);
        if compare == Ordering::Equal {
            if entry1u.status == "DELETED" && entry2u.status != "DELETED" {
                println!("DELETED {}", entry1u.path);
            } else if entry2u.status == "DELETED" && entry1u.status != "DELETED" {
                println!("DELETED {}", entry2u.path);
            } else if entry1u.size != entry2u.size {
                println!("CHANGED {}", entry1u.path);
            } else if entry1u.hash != entry2u.hash {
                println!("CHANGED {}", entry1u.path);
            } else if entry1u.timestamp != entry2u.timestamp {
                print!("TIME {}\t", entry1u.path);
                let d = UNIX_EPOCH + Duration::from_secs(entry1u.timestamp);
                // Create DateTime from SystemTime
                let datetime = DateTime::<Local>::from(d);
                // Formats the combined date and time with the specified format string.
                let timestamp_str = datetime.format("%Y%m%d%H%M.%S").to_string();
                println!{"touch -t {} {}/{}",timestamp_str,root2,entry2u.path};
            } else if entry1u.perms != entry2u.perms {
                println!("PERMS {}", entry1u.path);
            }
            //println!("MISSING {}", entry1.unwrap().path);
            entry1 = iter1.next();
            entry2 = iter2.next();
        } else if compare == Ordering::Less {
            if entry1u.status != "DELETED" {
                print!("MISSING {}\t", entry1u.path);
                println!("cp {}/{} {}/{}",root1.to_owned(),entry1u.path,root2.to_owned(),entry1u.path);
                //println!("Less {}", entry1.unwrap().path);
            }
            entry1 = iter1.next();
        } else if compare == Ordering::Greater {
            if entry2u.status != "DELETED" {
                print!("MISSING {}\t", entry2u.path);
                println!("cp {}/{} {}/{}",root2.to_owned(),entry2u.path,root1,entry2u.path);
                //println!("Greater {}", entry2.unwrap().path);
            }
            entry2 = iter2.next();
        }
    }

    while let Some(entry1u) = &entry1 {
        if entry1u.status != "DELETED" {
            print!("MISSING {}\t", entry1u.path);
            println!("cp {}/{} {}/{}",root1.to_owned(),entry1u.path,root2.to_owned(),entry1u.path);
            //println!("Less {}", entry1.unwrap().path);
        }
        entry1 = iter1.next();
    }

    while let Some(entry2u) = &entry2 {
        if entry2u.status != "DELETED" {
            print!("MISSING {}\t", entry2u.path);
            println!("cp {}/{} {}/{}",root2.to_owned(),entry2u.path,root1.to_owned(),entry2u.path);
            //println!("Greater {}", entry2.unwrap().path);
        }
        entry2 = iter2.next();
    }

    println!("Ending");
}