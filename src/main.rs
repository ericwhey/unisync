use std::cmp::Ordering;
use std::env::args;
use std::fs;
use std::fs::File;
use std::io::{Read, Write, BufReader, BufRead, self};
use std::path::{Path, self, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{SystemTime, Duration, UNIX_EPOCH};
use uuid::Uuid;
use chrono::{Utc, NaiveDateTime, Local};
use chrono::prelude::DateTime;
use std::os::unix::fs::PermissionsExt;

use sha2::{Sha256, Digest};
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
	let mut file = File::open(path).unwrap();
	let mut hasher = Sha256::new();
        io::copy(&mut file, &mut hasher).unwrap();
        let hash = hasher.finalize();
	//return hash.encode_hex::<String>();
    //return hash;
    self.hash = hex::encode(hash);
	//return hash.to_string();

    }
}

impl Entry {
    pub fn to_string(&self) -> String {
        return String::from(format!("{}\t{}\t{}\t{}\t{}\t{}", self.status, self.timestamp, self.size, self.perms, self.hash, self.path));
    }
}

fn scan(root: String, temp: Option<String>, tx:Sender<Entry>) {
    //let mut list = LinkedList::new();
    //let root = ".".to_owned();
    println!("Scanning {}", &root);
    let root_full = root.to_owned() + "/";
    let last_path = root_full.to_owned() + ".unisync/last.txt";
    let next_path: String;

    let id = Uuid::new_v4();
    if let Some(tempu) = temp {
        next_path = tempu + "/" + &id.to_string() + ".txt";
    } else {
        next_path = root_full.to_owned() + ".unisync/" + &id.to_string() + ".txt";
    }
    println!("Next path is {}", &next_path);
    println!("Root path is {}", &root_full);

    fs::create_dir_all(root_full.to_owned() + ".unisync").unwrap();

    let mut reader:BufReader<File>;
    if Path::new(last_path.as_str()).exists() {
        let mut file_done = false;
        let input = File::open(last_path.to_owned()).unwrap();
        reader = BufReader::new(input);

        let mut line = String::new();
        reader.read_line(&mut line).expect("Should work");
        let mut last_entry = Entry::new(line);

        let mut output = File::create(next_path.to_owned()).unwrap();
        let mut next_entry;
        for dir_entry in WalkDir::new(root.to_owned())
                .follow_links(false)        
                .sort_by_key(|a| a.file_name().to_owned())
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| !e.path().starts_with(root_full.to_owned() + ".unisync/"))
                .filter(|e| !e.path().starts_with(root_full.to_owned() + "#recycle/"))
                .filter(|e| !e.path().starts_with(root_full.to_owned() + "@eaDir/"))
                .filter(|e| !e.file_type().is_dir()) {

            //let path = dir_entry.path();
    
            next_entry = Entry::from_dir_entry(dir_entry.to_owned(), root_full.to_owned());
            //writeln!(output,"{}",next_entry.to_string());
            let mut compare = last_entry.path.cmp(&next_entry.path);
            
            while compare == Ordering::Less && !file_done {
                println!("Got LESS {} {}",last_entry.path,next_entry.path);
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
                println!("Got GREATER {} {}",last_entry.path,next_entry.path);
                let path = dir_entry.path();
                next_entry.hash_path(path);
                writeln!(output,"{}",next_entry.to_string());
                tx.send(next_entry.clone());
            } else if compare == Ordering::Equal {
                println!("Got EQUAL {} {}",last_entry.path,next_entry.path);
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
		println!("GOT FILE DONE {}", next_entry.path);
                next_entry.hash_path(dir_entry.path());
                writeln!(output,"{}",next_entry.to_string());
                tx.send(next_entry.clone());
            }
        }
        while !file_done {
	    println!("GOT DELETED {}", last_entry.path);
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
	println!("Trying to output first time");

        for entry in WalkDir::new(root)
                .follow_links(false)
                .sort_by_key(|a| a.file_name().to_owned())
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| !e.path().starts_with(root_full.to_owned() + ".unisync/"))
                .filter(|e| !e.path().starts_with(root_full.to_owned() + "#recycle/"))
                .filter(|e| !e.path().starts_with(root_full.to_owned() + "@eaDir/"))
                .filter(|e| !e.file_type().is_dir()) {
    
            let mut next_entry = Entry::from_dir_entry(entry.to_owned(), root_full.to_owned());
            next_entry.hash_path(entry.path());
            println!("Next line is {}", next_entry.to_string());
            writeln!(output,"{}",next_entry.to_string());
            tx.send(next_entry.clone());
        }

        fs::rename(next_path, last_path);
    }

}

fn main() {
    let mut temp = None;
    let mut root1 = None;
    let mut root2 = None;

    let args: Vec<String> = args().collect();

    let mut argsIter = args.iter();

    let mut arg = argsIter.next();

    let mut index = 0;

    while let Some(argu) = arg {
        if argu == "--temp" {
            let tempArg = argsIter.next();
            if let Some(temp_argu) = tempArg {
                temp = Some(String::from(temp_argu));
            } else {
                println!("Could not unwrap temp arg");
            }
            
        } else {
            if index == 1 {
                root1 = Some(String::from(argu));
            } else if index == 2 {
                root2 = Some(String::from(argu));
            }
            println!("Going through args main loop {}", &index);
            index += 1;
            
        }
        arg = argsIter.next();

    }


    if let Some(root1u) = root1 {
        //let root1c = root1.clone();
        println!("First volume is {}", root1u);
        let (tx1, rx1) = mpsc::channel();
        let temp1c = temp.clone();
        let root1c = root1u.clone();
        
        let thread_join_handle = thread::spawn(move || {
            scan(root1c,temp1c, tx1);
        });

        if let Some(root2u) = root2 {
            println!("Second volume is {}", root2u);
            let (tx2, rx2) = mpsc::channel();
            let temp2c = temp.clone();
            let root2c = root2u.clone();
            thread::spawn(move || {
                scan(root2c, temp2c, tx2);
            });

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
                        println!{"touch -t {} {}/{}",timestamp_str,root2u.to_owned(),entry2u.path};
                    } else if entry1u.perms != entry2u.perms {
                        println!("PERMS {}", entry1u.path);
                    }
                    //println!("MISSING {}", entry1.unwrap().path);
                    entry1 = iter1.next();
                    entry2 = iter2.next();
                } else if compare == Ordering::Less {
                    if entry1u.status != "DELETED" {
                        print!("MISSING {}\t", entry1u.path);
                        println!("cp {}/{} {}/{}",root1u.to_owned(),entry1u.path,root2u.to_owned(),entry1u.path);
                        //println!("Less {}", entry1.unwrap().path);
                    }
                    entry1 = iter1.next();
                } else if compare == Ordering::Greater {
                    if entry2u.status != "DELETED" {
                        print!("MISSING {}\t", entry2u.path);
                        println!("cp {}/{} {}/{}",root2u.to_owned(),entry2u.path,root1u.to_owned(),entry2u.path);
                        //println!("Greater {}", entry2.unwrap().path);
                    }
                    entry2 = iter2.next();
                }
            }

            while let Some(entry1u) = &entry1 {
                if entry1u.status != "DELETED" {
                    print!("MISSING {}\t", entry1u.path);
                    println!("cp {}/{} {}/{}",root1u.to_owned(),entry1u.path,root2u.to_owned(),entry1u.path);
                    //println!("Less {}", entry1.unwrap().path);
                }
                entry1 = iter1.next();
            }

            while let Some(entry2u) = &entry2 {
                if entry2u.status != "DELETED" {
                    print!("MISSING {}\t", entry2u.path);
                    println!("cp {}/{} {}/{}",root2u.to_owned(),entry2u.path,root1u.to_owned(),entry2u.path);
                    //println!("Greater {}", entry2.unwrap().path);
                }
                entry2 = iter2.next();
            }

            println!("Ending");
        } else {
            thread_join_handle.join();
        }
    }
}
