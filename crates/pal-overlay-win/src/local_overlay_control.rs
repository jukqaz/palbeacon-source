use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

use pal_build_contract::CURRENT_GAME_BUILD_ID_U64;
use pal_overlay_control::{
    OverlayControlDocument, OverlayPoiKind, default_control_path, read_control,
    read_or_create_control, read_poi_catalog,
};

use crate::actual_map_preview::{
    MapPoi, MapPoiIcon, MapPoiKind, MapRaster, authoritative_main_map_world_to_image,
};

// Keep app-side zoom, filter, and interaction changes feeling immediate without involving the
// renderer thread in file I/O. The watcher performs only a metadata check at this cadence.
const WATCH_INTERVAL: Duration = Duration::from_millis(16);

pub struct LocalOverlayControlWatcher {
    receiver: Receiver<OverlayControlDocument>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LocalOverlayControlWatcher {
    pub fn start() -> Result<(OverlayControlDocument, Self), pal_overlay_control::ControlError> {
        let path = default_control_path();
        let initial = read_or_create_control(&path)?;
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("pal-overlay-control-watch".to_owned())
            .spawn(move || {
                let mut signature = file_signature(&path);
                while !worker_stop.load(Ordering::Acquire) {
                    thread::sleep(WATCH_INTERVAL);
                    let current = file_signature(&path);
                    if current == signature {
                        continue;
                    }
                    signature = current;
                    match read_control(&path) {
                        Ok(document) => {
                            if sender.send(document).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            eprintln!(
                                "Overlay control update was ignored; the previous settings remain active: {error}"
                            );
                        }
                    }
                }
            })?;
        Ok((
            initial,
            Self {
                receiver,
                stop,
                worker: Some(worker),
            },
        ))
    }

    pub fn take_latest(&self) -> Option<OverlayControlDocument> {
        let mut latest = None;
        loop {
            match self.receiver.try_recv() {
                Ok(document) => latest = Some(document),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return latest,
            }
        }
    }
}

impl Drop for LocalOverlayControlWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn load_projected_pois(
    path: Option<&Path>,
    map: &MapRaster,
) -> Result<Vec<MapPoi>, pal_overlay_control::ControlError> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    let catalog = read_poi_catalog(path, CURRENT_GAME_BUILD_ID_U64)?;
    let transform = authoritative_main_map_world_to_image();
    let mut projected = Vec::with_capacity(catalog.pois.len());
    let catalog_root = path.parent().unwrap_or_else(|| Path::new("."));
    let mut icon_cache: HashMap<PathBuf, Arc<MapPoiIcon>> = HashMap::new();
    for poi in catalog.pois {
        let Some(point) = transform.project_within_bounds(poi.world_x, poi.world_y) else {
            continue;
        };
        let kind = match poi.kind {
            OverlayPoiKind::FastTravel => MapPoiKind::FastTravel,
            OverlayPoiKind::Boss => MapPoiKind::Boss,
            OverlayPoiKind::Dungeon => MapPoiKind::Dungeon,
        };
        if let Some(mut projected_poi) = MapPoi::new(
            point.x() * f64::from(map.width()),
            point.y() * f64::from(map.height()),
            kind,
        ) {
            if kind == MapPoiKind::Boss
                && let Some(icon_file) = poi.icon_file
            {
                let icon_path = catalog_root.join(icon_file);
                let icon = icon_cache.get(&icon_path).cloned().or_else(|| {
                    let decoded = fs::read(&icon_path)
                        .ok()
                        .and_then(|bytes| MapPoiIcon::decode_png(&bytes))
                        .map(Arc::new)?;
                    icon_cache.insert(icon_path, Arc::clone(&decoded));
                    Some(decoded)
                });
                if let Some(icon) = icon {
                    projected_poi = projected_poi.with_icon(icon);
                }
            }
            projected.push(projected_poi);
        }
    }
    Ok(projected)
}

fn file_signature(path: &PathBuf) -> Option<(u64, SystemTime)> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| Some((metadata.len(), metadata.modified().ok()?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_poi_path_is_an_empty_catalog() {
        let map = MapRaster::new(1, 1, vec![0]).expect("map");
        assert!(load_projected_pois(None, &map).expect("empty").is_empty());
    }
}
