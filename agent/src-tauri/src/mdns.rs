use mdns_sd::{ServiceDaemon, ServiceInfo};

pub fn register(pc_name: &str, port: u16) -> Option<ServiceDaemon> {
    let mdns = ServiceDaemon::new().ok()?;
    let ip = local_ip_address::local_ip().ok()?.to_string();
    let host_name = format!("{ip}.local.");
    let properties: [(&str, &str); 1] = [("v", "1")];
    let service_info = ServiceInfo::new(
        "_autodeck._tcp.local.",
        pc_name,
        &host_name,
        ip.as_str(),
        port,
        &properties[..],
    )
    .ok()?;
    mdns.register(service_info).ok()?;
    Some(mdns)
}
