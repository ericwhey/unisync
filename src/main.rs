use std::cmp::Ordering;
use std::collections::LinkedList;
use std::env::args;
use std::fs;
use std::fs::File;
use std::io::{Write, BufReader, BufRead, self};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{SystemTime, Duration, UNIX_EPOCH};
use std::os::unix::fs::PermissionsExt;
use fs_extra::{dir,file};
use uuid::Uuid;
use chrono::Local;
use chrono::prelude::DateTime;
use console::{Term, Key};

use log::{debug, error, log_enabled, info, Level};

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

pub struct Difference {
    pub path: String,
    pub status: String,
    pub path_type: String,
    pub side: i32,
    pub path1: String,
    pub path2: String
}

fn compress_dirs(path1: Option<&String>, path2: Option<&String>, pathMissing: &String) -> Option<String> {
    //println!("What is missing");
    let mut one = None;
    let mut two = None;
    if let Some(path1u) = path1 {
        one = Path::new(path1u).parent();
    }
    if let Some(path2u) = path2 {
        two = Path::new(path2u).parent();
    }
    let mut missing = Path::new(pathMissing).parent();
    //println!("Missing {}",missing.unwrap().to_string_lossy());
    let mut lastMissing = None;
    let mut done =false;
    //missing =  missing.unwrap().parent();
    //println!("Missing {}",missing.unwrap().to_string_lossy());
    lastMissing = None;
    while !done {
        //println!("Missing {}",missing.unwrap().to_string_lossy());
        if missing.is_none() {
            done = true;
        } else if let Some(one_u) = one {
            if one_u.starts_with(missing.unwrap()) {
                done = true;
            }
        } else if let Some(two_u) = two {
            if two_u.starts_with(missing.unwrap()) {
                done = true;
            }
        }
        if !done {
            lastMissing = missing;
            missing =  missing.unwrap().parent();
        }
        
    }
    if let Some(lastMissingU) = lastMissing {
        return Some(String::from(lastMissingU.to_string_lossy()));
    }
    return None;
}

fn scan(root: String, temp: &Option<String>, tx:Sender<Entry>) {
    //let mut list = LinkedList::new();
    //let root = ".".to_owned();
    info!("Scanning {}", &root);
    let root_full = root.to_owned() + "/";
    let last_path = root_full.to_owned() + ".unisync/last.txt";
    let next_path: String;

    let id = Uuid::new_v4();
    if let Some(tempu) = temp {
        next_path = String::from(tempu) + "/" + &id.to_string() + ".txt";
    } else {
        next_path = root_full.to_owned() + ".unisync/" + &id.to_string() + ".txt";
    }
    info!("Next path is {}", &next_path);
    info!("Root path is {}", &root_full);

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
                info!("Got LESS {} {}",last_entry.path,next_entry.path);
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
                info!("Got GREATER {} {}",last_entry.path,next_entry.path);
                let path = dir_entry.path();
                next_entry.hash_path(path);
                writeln!(output,"{}",next_entry.to_string());
                tx.send(next_entry.clone());
            } else if compare == Ordering::Equal {
                info!("Got EQUAL {} {}",last_entry.path,next_entry.path);
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
		        info!("GOT FILE DONE {}", next_entry.path);
                next_entry.hash_path(dir_entry.path());
                writeln!(output,"{}",next_entry.to_string());
                tx.send(next_entry.clone());
            }
        }
        while !file_done {
	        info!("GOT DELETED {}", last_entry.path);
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
            info!("Next line is {}", next_entry.to_string());
            writeln!(output,"{}",next_entry.to_string());
            tx.send(next_entry.clone());
        }

        fs::rename(next_path, last_path);
    }

}

fn main() {
    env_logger::init();

    let out = true;
    let mut temp = None;
    let mut no_perms = false;
    let mut no_times = false;
    let mut no_compress = false;
    let mut root1 = None;
    let mut root2 = None;

    let args: Vec<String> = args().collect();

    let mut argsIter = args.iter();

    let mut arg = argsIter.next();

    let mut index = 0;

    while let Some(argu) = arg {
        if argu == "--temp" {
            let tempArg = argsIter.next();
            match tempArg {
                Some(tempArgU) => {
                    temp = Some(tempArgU.clone());
                },
                None => {

                }
            }
        } else if argu == "--noperms" {
            no_perms = true;
        } else if argu == "--notimes" {
            no_times = true;
        } else if argu == "--nocompress" {
            no_compress = true;
        } else {
            if index == 1 {
                root1 = Some(String::from(argu));
            } else if index == 2 {
                root2 = Some(String::from(argu));
            }
            info!("Going through args main loop {}", &index);
            index += 1;
            
        }
        arg = argsIter.next();

    }


    if let Some(root1u) = root1 {
        //let root1c = root1.clone();
        info!("First volume is {}", root1u);
        let (tx1, rx1) = mpsc::channel();
        let temp1c = temp.clone();
        
        let root1uc = root1u.clone();
        let thread_join_handle = thread::spawn(move || {
            scan(root1uc, &temp1c, tx1);
        });

        if let Some(root2u) = root2 {
            info!("Second volume is {}", root2u);
            let (tx2, rx2) = mpsc::channel();
            let temp2c = temp.clone();
            let root2uc = root2u.clone();
            thread::spawn( move || {
                scan(root2uc, &temp2c, tx2);
            });

            let mut iter1 = rx1.iter();
            let mut iter2 = rx2.iter();

            let mut previousPath1: Option<&String> = None;
            let mut previousPath2: Option<&String> = None;

            let mut entry1uPath: String;
            let mut entry2uPath: String;

            let mut entry1 = iter1.next();
            let mut entry2 = iter2.next();

            let mut last_dir = None;

            let mut differences: LinkedList<Difference> = LinkedList::new();

            info!("Starting");


            while let (Some(entry1u),Some(entry2u)) = (&entry1, &entry2) {

    
                let compare = entry1u.path.cmp(&entry2u.path);
                if compare == Ordering::Equal {
                    if entry1u.status == "DELETED" && entry2u.status == "DELETED" {
                    } else if entry1u.status == "DELETED" && entry2u.status != "DELETED" {
                        println!("DELETED {}", entry1u.path);
                        differences.push_back(Difference {path:entry1u.path.to_owned(), path_type: String::from("FILE"), status: String::from("DELETED"), side: 1, path1: root1u.to_owned() + "/" + entry1u.path.as_str(), path2: root2u.to_owned() + "/" + entry2u.path.as_str()  });
                    } else if entry2u.status == "DELETED" && entry1u.status != "DELETED" {
                        println!("DELETED {}", entry2u.path);
                        differences.push_back(Difference {path:entry1u.path.to_owned(), path_type: String::from("FILE"), status: String::from("DELETED"), side: 2, path1: root1u.to_owned() + "/" + entry1u.path.as_str(), path2: root2u.to_owned() + "/" + entry2u.path.as_str()  });
                    } else if entry1u.size != entry2u.size {
                        println!("CHANGED {}", entry1u.path);
                        differences.push_back(Difference {path:entry1u.path.to_owned(), path_type: String::from("FILE"), status: String::from("CHANGED"), side: 0, path1: root1u.to_owned() + "/" + entry1u.path.as_str(), path2: root2u.to_owned() + "/" + entry2u.path.as_str()  });
                    } else if entry1u.hash != entry2u.hash {
                        println!("CHANGED {}", entry1u.path);
                        differences.push_back(Difference {path:entry1u.path.to_owned(), path_type: String::from("FILE"), status: String::from("CHANGED"), side: 0, path1: root1u.to_owned() + "/" + entry1u.path.as_str(), path2: root2u.to_owned() + "/" + entry2u.path.as_str()  });
                    } else if !no_times && entry1u.timestamp != entry2u.timestamp {
                        print!("TIME {}\t", entry1u.path);
                        let d = UNIX_EPOCH + Duration::from_secs(entry1u.timestamp);
                        // Create DateTime from SystemTime
                        let datetime = DateTime::<Local>::from(d);
                        // Formats the combined date and time with the specified format string.
                        let timestamp_str = datetime.format("%Y%m%d%H%M.%S").to_string();
                        println!{"touch -t {} {}/{}",timestamp_str,root2u.to_owned(),entry2u.path};
                        differences.push_back(Difference {path:entry1u.path.to_owned(), path_type: String::from("FILE"), status: String::from("TIME"), side: 1, path1: root1u.to_owned() + "/" + entry1u.path.as_str(), path2: root2u.to_owned() + "/" + entry2u.path.as_str()  });
                    } else if !no_perms && entry1u.perms != entry2u.perms {
                        println!("PERMS {}", entry1u.path);
                        differences.push_back(Difference {path:entry1u.path.to_owned(), path_type: String::from("FILE"), status: String::from("TIME"), side: 1, path1: root1u.to_owned() + "/" + entry1u.path.as_str(), path2: root2u.to_owned() + "/" + entry2u.path.as_str()  });
                    }
                    //println!("MISSING {}", entry1.unwrap().path);
                    entry1uPath = entry1u.path.to_string();
                    entry2uPath = entry2u.path.to_string();
                    previousPath1 = Some(&entry1uPath);
                    previousPath2 = Some(&entry2uPath);
                    entry1 = iter1.next();
                    entry2 = iter2.next();
                } else if compare == Ordering::Less {
                    if entry1u.status != "DELETED" {
                        if !no_compress {
                            match compress_dirs(previousPath2, Some(&entry2u.path), &entry1u.path) {
                                Some(dir_u) => {
                                    if let Some(last_dir_u) = last_dir {
                                        if dir_u != last_dir_u {
                                            print!("MISSING {}\t", dir_u);
                                            println!("cp -R {}/{} {}/{}",root1u.to_owned(),dir_u,root2u.to_owned(),dir_u);
                                            differences.push_back(Difference {path:String::from(&dir_u), path_type: String::from("DIR"), status: String::from("MISSING"), side: 1, path1: root1u.to_owned() + "/" + String::from(&dir_u).as_str(), path2: root2u.to_owned() + "/" + String::from(&dir_u).as_str()  });
                                        }
                                    } else {
                                        print!("MISSING {}\t", dir_u);
                                        println!("cp -R {}/{} {}/{}",root1u.to_owned(),dir_u,root2u.to_owned(),dir_u);
                                        differences.push_back(Difference {path:String::from(&dir_u), path_type: String::from("DIR"), status: String::from("MISSING"), side: 1, path1: root1u.to_owned() + "/" + String::from(&dir_u).as_str(), path2: root2u.to_owned() + "/" + String::from(&dir_u).as_str()  });
                                    }
                                    last_dir = Some(String::from(dir_u));
                                }
                                None => {
                                    print!("MISSING {}\t", entry1u.path);
                                    println!("cp {}/{} {}/{}",root1u.to_owned(),entry1u.path,root2u.to_owned(),entry1u.path);
                                    differences.push_back(Difference {path:entry1u.path.to_owned(), path_type: String::from("FILE"), status: String::from("MISSING"), side: 1, path1: root1u.to_owned() + "/" + entry1u.path.as_str(), path2: root2u.to_owned() + "/" + entry1u.path.as_str()  });
                                }
                            }
                        } else {
                            print!("MISSING {}\t", entry1u.path);
                            println!("cp {}/{} {}/{}",root1u.to_owned(),entry1u.path,root2u.to_owned(),entry1u.path);
                            differences.push_back(Difference {path:entry1u.path.to_owned(), path_type: String::from("FILE"), status: String::from("MISSING"), side: 1, path1: root1u.to_owned() + "/" + entry1u.path.as_str(), path2: root2u.to_owned() + "/" + entry1u.path.as_str()  });
                        }
                        //println!("Next numAncestors {:?}",numAncestors(Path::new(&entry1u.path), Path::new(&entry2u.path.clone())));
                        
                        //print!("MISSING {}\t", entry1u.path);
                        //println!("cp {}/{} {}/{}",root1u.to_owned(),entry1u.path,root2u.to_owned(),entry1u.path);
                        //println!("Less {}", entry1.unwrap().path);
                    }
                    entry1uPath = entry1u.path.to_string();
                    previousPath1 = Some(&entry1uPath);
                    entry1 = iter1.next();
                } else if compare == Ordering::Greater {
                    if entry2u.status != "DELETED" {
                        if !no_compress {
                            match compress_dirs(previousPath1, Some(&entry1u.path), &entry2u.path) {
                                Some(dir_u) => {
                                    if let Some(last_dir_u) = last_dir {
                                        if dir_u != last_dir_u {
                                            print!("MISSING {}\t", dir_u);
                                            println!("cp -R {}/{} {}/{}",root2u.to_owned(),dir_u,root1u.to_owned(),dir_u);
                                            differences.push_back(Difference {path:String::from(&dir_u), path_type: String::from("DIR"), status: String::from("MISSING"), side: 2, path1: root1u.to_owned() + "/" + String::from(&dir_u).as_str(), path2: root2u.to_owned() + "/" + String::from(&dir_u).as_str()  });
                                        }
                                    } else {
                                        print!("MISSING {}\t", dir_u);
                                        println!("cp -R {}/{} {}/{}",root2u.to_owned(),dir_u,root1u.to_owned(),dir_u);
                                        differences.push_back(Difference {path:String::from(&dir_u), path_type: String::from("DIR"), status: String::from("MISSING"), side: 2, path1: root1u.to_owned() + "/" + String::from(&dir_u).as_str(), path2: root2u.to_owned() + "/" + String::from(&dir_u).as_str()  });
                                    }
                                    last_dir = Some(String::from(dir_u));
                                }
                                None => {
                                    print!("MISSING {}\t", entry2u.path);
                                    println!("cp {}/{} {}/{}",root2u.to_owned(),entry2u.path,root1u.to_owned(),entry2u.path);
                                    differences.push_back(Difference {path:entry2u.path.to_owned(), path_type: String::from("FILE"), status: String::from("MISSING"), side: 2, path1: root1u.to_owned() + "/" + entry2u.path.as_str(), path2: root2u.to_owned() + "/" + entry2u.path.as_str()  });
                                }
                            }
                        } else {
                            print!("MISSING {}\t", entry2u.path);
                            println!("cp {}/{} {}/{}",root2u.to_owned(),entry2u.path,root1u.to_owned(),entry2u.path);
                            differences.push_back(Difference {path:entry2u.path.to_owned(), path_type: String::from("FILE"), status: String::from("MISSING"), side: 2, path1: root1u.to_owned() + "/" + entry2u.path.as_str(), path2: root2u.to_owned() + "/" + entry2u.path.as_str()  });
                        }
                        //println!("Greater {}", entry2.unwrap().path);
                    }
                    entry2uPath = entry2u.path.to_string();
                    previousPath2 = Some(&entry2uPath);
                    entry2 = iter2.next();
                }

            }

            while let Some(entry1u) = &entry1 {
                if entry1u.status != "DELETED" {
                    if !no_compress {
                        match compress_dirs(previousPath2, None, &entry1u.path) {
                            Some(dir_u) => {
                                if let Some(last_dir_u) = last_dir {
                                    if dir_u != last_dir_u {
                                        print!("MISSING {}\t", dir_u);
                                        println!("cp -R {}/{} {}/{}",root1u.to_owned(),dir_u,root2u.to_owned(),dir_u);
                                        differences.push_back(Difference {path:String::from(&dir_u), path_type: String::from("DIR"), status: String::from("MISSING"), side: 1, path1: root1u.to_owned() + "/" + String::from(&dir_u).as_str(), path2: root2u.to_owned() + "/" + String::from(&dir_u).as_str()  });
                                    }
                                } else {
                                    print!("MISSING {}\t", dir_u);
                                    println!("cp -R {}/{} {}/{}",root1u.to_owned(),dir_u,root2u.to_owned(),dir_u);
                                    differences.push_back(Difference {path:String::from(&dir_u), path_type: String::from("DIR"), status: String::from("MISSING"), side: 1, path1: root1u.to_owned() + "/" + String::from(&dir_u).as_str(), path2: root2u.to_owned() + "/" + String::from(&dir_u).as_str()  });
                                }
                                last_dir = Some(String::from(dir_u));
                            }
                            None => {
                                print!("MISSING {}\t", entry1u.path);
                                println!("cp {}/{} {}/{}",root1u.to_owned(),entry1u.path,root2u.to_owned(),entry1u.path);
                                differences.push_back(Difference {path:entry1u.path.to_owned(), path_type: String::from("FILE"), status: String::from("MISSING"), side: 1, path1: root1u.to_owned() + "/" + entry1u.path.as_str(), path2: root2u.to_owned() + "/" + entry1u.path.as_str()  });
                            }
                        }
                    } else {
                        print!("MISSING {}\t", entry1u.path);
                        println!("cp {}/{} {}/{}",root1u.to_owned(),entry1u.path,root2u.to_owned(),entry1u.path);
                        differences.push_back(Difference {path:entry1u.path.to_owned(), path_type: String::from("FILE"), status: String::from("MISSING"), side: 1, path1: root1u.to_owned() + "/" + entry1u.path.as_str(), path2: root2u.to_owned() + "/" + entry1u.path.as_str()  });
                    }
                    //print!("MISSING {}\t", entry1u.path);
                    
                    //println!("Less {}", entry1.unwrap().path);
                }
                entry1uPath = entry1u.path.to_string();
                previousPath1 = Some(&entry1uPath);;
                entry1 = iter1.next();
            }

            while let Some(entry2u) = &entry2 {
                if entry2u.status != "DELETED" {
                    if !no_compress {
                        //if let Some(previousPath1u) = &previousPath1 {
                            match compress_dirs(previousPath1, None, &entry2u.path) {
                                Some(dir_u) => {
                                    if let Some(last_dir_u) = last_dir {
                                        if dir_u != last_dir_u {
                                            print!("MISSING {}\t", dir_u);
                                            println!("cp -R {}/{} {}/{}",root2u.to_owned(),dir_u,root1u.to_owned(),dir_u);
                                            differences.push_back(Difference {path:String::from(&dir_u), path_type: String::from("DIR"), status: String::from("MISSING"), side: 2, path1: root1u.to_owned() + "/" + String::from(&dir_u).as_str(), path2: root2u.to_owned() + "/" + String::from(&dir_u).as_str()  });
                                        }
                                    } else {
                                        print!("MISSING {}\t", dir_u);
                                        println!("cp -R {}/{} {}/{}",root2u.to_owned(),dir_u,root1u.to_owned(),dir_u);
                                        differences.push_back(Difference {path:String::from(&dir_u), path_type: String::from("DIR"), status: String::from("MISSING"), side: 2, path1: root1u.to_owned() + "/" + String::from(&dir_u).as_str(), path2: root2u.to_owned() + "/" + String::from(&dir_u).as_str()  });
                                    }
                                    last_dir = Some(String::from(dir_u));
                                }
                                None => {
                                    print!("MISSING {}\t",entry2u.path);
                                    println!("cp {}/{} {}/{}",root2u.to_owned(),entry2u.path,root1u.to_owned(),entry2u.path);
                                    differences.push_back(Difference {path:entry2u.path.to_owned(), path_type: String::from("FILE"), status: String::from("MISSING"), side: 2, path1: root1u.to_owned() + "/" + entry2u.path.as_str(), path2: root2u.to_owned() + "/" + entry2u.path.as_str()  });
                                }
                            }
                        /* } else {
                            println!("OOPs");
                        }*/
                    } else {
                        print!("MISSING {}\t",entry2u.path);
                        println!("cp {}/{} {}/{}",root2u.to_owned(),entry2u.path,root1u.to_owned(),entry2u.path);
                        differences.push_back(Difference {path:entry2u.path.to_owned(), path_type: String::from("FILE"), status: String::from("MISSING"), side: 2, path1: root1u.to_owned() + "/" + entry2u.path.as_str(), path2: root2u.to_owned() + "/" + entry2u.path.as_str()  });
                    }
                    //print!("MISSING {}\t", entry2u.path);
                    
                    //println!("Greater {}", entry2.unwrap().path);
                }
                //entry2uPath = entry2u.path.to_string();
                //previousPath2 = Some(&entry2uPath);
                entry2 = iter2.next();
            }

            info!("Ending");

            let mut iter = differences.iter();

            let term = Term::stdout();

            for difference in iter {
                println!("Difference {} {} {} {}", difference.status, difference.path, difference.path1, difference.path2);
                
                let key_result = term.read_key();
                match key_result {
                    Ok(key) => {
                        if key == Key::Enter {
                            println!("CR pressed");
                            if difference.status == "MISSING" {
                                if difference.path_type == "DIR" {
                                    println!("MISSING DIR {}", difference.side);
                                    if difference.side == 1 {
                                        let options = dir::CopyOptions::new(); //Initialize default values for CopyOptions
                                        let path2 = Path::new(&difference.path2).parent();
                                        let copy_result = dir::copy(&difference.path1, path2.unwrap(), &options);
                                        match copy_result {
                                            Ok(result) => {},
                                            Err(error) => println!("Problem opening the dir: {:?}", error),
                                        };
                                    } else if difference.side == 2 {
                                        let options = dir::CopyOptions::new(); //Initialize default values for CopyOptions
                                        let path1 = Path::new(&difference.path1).parent();
                                        let copy_result = dir::copy(&difference.path2, path1.unwrap(), &options);
                                        match copy_result {
                                            Ok(result) => {},
                                            Err(error) => println!("Problem opening the dir: {:?}", error),
                                        };
                                    }
                                } else if difference.path_type == "FILE" {
                                    println!("MISSING FILE {}", difference.side);
                                    if difference.side == 1 {
                                        let options = file::CopyOptions::new(); //Initialize default values for CopyOptions
                                        let path2 = Path::new(&difference.path2).parent();
                                        let copy_result = file::copy(&difference.path1, &difference.path2, &options);
                                        match copy_result {
                                            Ok(result) => {},
                                            Err(error) => println!("Problem opening the file: {:?}", error),
                                        };
                                    } else if difference.side == 2 {
                                        let options = file::CopyOptions::new(); //Initialize default values for CopyOptions
                                        let path1 = Path::new(&difference.path1).parent();
                                        let copy_result = file::copy(&difference.path2, &difference.path1, &options);
                                        match copy_result {
                                            Ok(result) => {},
                                            Err(error) => println!("Problem opening the file: {:?}", error),
                                        };
                                    }
                                }
                            }
                        } else if key == Key::Char(char::from_u32(65).unwrap()) {
                            println!("A pressed");
                        } else if key == Key::Char(char::from('/')) {
                            println!("Skipped");
                        }
                    }
                    Error => {}
                }
                
            }
        } else {
            //thread_join_handle.join();
            for entry in rx1.iter() {
                if out {
                    println!("{}", entry.to_string());
                }

            }
        }
    }
}
