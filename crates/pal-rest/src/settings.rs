#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServerSettings {
    pub experience_rate: f64,
    pub capture_rate: f64,
    pub pal_spawn_rate: f64,
    pub collection_drop_rate: f64,
    pub collection_object_hp_rate: f64,
    pub collection_respawn_rate: f64,
    pub enemy_drop_rate: f64,
    pub base_worker_limit: u64,
    pub egg_hatching_hours: f64,
    pub work_speed_rate: f64,
    pub fast_travel_enabled: bool,
}
