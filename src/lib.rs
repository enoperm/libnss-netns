use std::process::Command;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use libnss::host::{HostHooks, Host, Addresses, AddressFamily};
use libnss::interop::Response;

use serde::Deserialize;

#[macro_use]
extern crate lazy_static;

struct NsHost;

#[derive(Deserialize)]
struct LinkAddress {
    pub family: String,
    pub local: String,
    pub scope: String,
}

#[derive(Deserialize)]
struct LinkInfo {
    pub link_type: String,
    pub addr_info: Vec::<LinkAddress>,
}

#[derive(Deserialize)]
struct Netns {
    pub name: String,
}

libnss::libnss_host_hooks!(netns, NsHost);

impl HostHooks for NsHost {
    fn get_all_entries() -> libnss::interop::Response::<Vec::<Host>> {
        fn get_for_ns(name: &str) -> Vec::<Host> {
            let recv4 = NsHost::get_host_by_name(name, AddressFamily::IPv4);
            let recv6 = NsHost::get_host_by_name(name, AddressFamily::IPv6);

            let mut host_recs = vec![];

            if let Response::Success(rec) = recv4 {
                host_recs.push(rec);
            }

            if let Response::Success(rec) = recv6 {
                host_recs.push(rec);
            }

            host_recs
        }

        let ns_names = ip_netns_ls();
        if let Err(err) = ns_names {
            eprintln!("get_all_entries: {}", err);
            return Response::Unavail;
        }

        let ns_names = ns_names.unwrap();
        let hosts: Vec::<Host> = ns_names.into_iter().map(|ns| get_for_ns(&ns)).flatten().collect();

        Response::Success(hosts)
    }

    fn get_host_by_name(name: &str, family: libnss::host::AddressFamily) -> libnss::interop::Response<Host> {
        let result = ip_addr(Some(name));
        if let Err(err) = result {
            eprintln!("get_host_by_name: {}", err);
            return Response::NotFound;
        }

        let af_filter = match family {
            libnss::host::AddressFamily::IPv6 => "inet6",
            libnss::host::AddressFamily::IPv4 | _ => "inet",
        };

        let result = result.unwrap();

        let addresses: Vec::<IpAddr> =
            result
            .into_iter()
            .filter(|info| info.link_type != "loopback")
            .map(|link: LinkInfo| {
                let addr_info: Vec::<IpAddr> =
                    link
                    .addr_info
                    .into_iter()
                    .filter(|addr| addr.family == af_filter)
                    .filter(|addr| addr.scope != "link")
                    .map(|addr| {
                        let parsed: IpAddr = addr.local.parse().expect("not an IP address");
                        parsed
                    })
                    .collect();
                addr_info
            })
            .flatten()
            .collect()
        ;

        match family {
            AddressFamily::IPv6 => {
                let addresses: Vec::<Ipv6Addr> = addresses.into_iter().map(|a| match a {
                    IpAddr::V6(a) => a,
                    _ => unreachable!(),
                }).collect();

                if addresses.is_empty() {
                    return Response::NotFound;
                }

                Response::Success(Host{
                    name: name.into(),
                    aliases: vec![],
                    addresses: Addresses::V6(addresses),
                })
            },

            AddressFamily::IPv4 | _ => {
                let addresses: Vec::<Ipv4Addr> = addresses.into_iter().map(|a| match a {
                    IpAddr::V4(a) => a,
                    _ => unreachable!(),
                }).collect();

                if addresses.is_empty() {
                    return Response::NotFound;
                }

                Response::Success(Host{
                    name: name.into(),
                    aliases: vec![],
                    addresses: Addresses::V4(addresses),
                })
            },
        }
    }

    fn get_host_by_addr(addr: std::net::IpAddr) -> libnss::interop::Response<Host> {
        let all_entries_response = NsHost::get_all_entries();
        if let Response::Success(all_entries) = all_entries_response {
            let is_v6 = addr.is_ipv6();

            let mut query = all_entries.into_iter().filter_map(|host| {
                match host.addresses {
                    Addresses::V6(ref addresses) => if is_v6 {
                        if let IpAddr::V6(addr) = addr {
                            if addresses.contains(&addr) {
                                Some(host)
                            } else { None }
                        } else { None }
                    } else { None },

                    Addresses::V4(ref addresses) => if !is_v6 {
                        if let IpAddr::V4(addr) = addr {
                            if addresses.contains(&addr) {
                                Some(host)
                            } else { None }
                        } else { None }
                    } else { None },
                }
            });

            match query.next() {
                None => Response::NotFound,
                Some(host) => Response::Success(host),
            }
        } else {
            Response::Unavail
        }
    }
}

fn ip_addr(netns: Option<&str>) -> Result::<Vec::<LinkInfo>, Box::<dyn std::error::Error>> {
    let args = {
        let mut args = vec!["-json".to_string()];

        if let Some(netns) = netns {
            args.extend(vec!["-n".into(), netns.into()].into_iter())
        }

        args.push("address".into());

        args
    };

    let result =
        Command::new("ip")
            .args(args)
            .output()
    ;

    let result: Vec::<LinkInfo> = match result {
        Ok(output) => {
            let as_string = String::from_utf8(output.stdout)?;
            let as_unstructured: serde_json::Value = serde_json::from_str(&as_string)?;
            let as_unstructured_arr = as_unstructured.as_array().ok_or("failed to parse JSON as array")?;
            let link_infos: Vec::<LinkInfo> = as_unstructured_arr.into_iter().map(|interface_json| {
                let link_info: LinkInfo = serde_json::from_value(interface_json.clone())?;
                Ok(link_info)
            })
            .filter_map(|info: Result<LinkInfo, Box::<dyn std::error::Error>>| {
                match info {
                    Ok(info) => Some(info),

                    Err(err) => {
                        eprintln!("ip_addr: {:?}: {}", netns, err);
                        None
                    },
                }
            })
            .collect();

            link_infos
        },

        Err(error) => { return Err(Box::new(error)); }
    };

    
    Ok(result)
}

fn ip_netns_ls() -> Result::<Vec::<String>, Box::<dyn std::error::Error>> {
    let args = vec!["-json".to_string(), "netns".into(), "ls".into()];

    let result =
        Command::new("ip")
            .args(args)
            .output()
    ;

    let result: Vec::<String> = match result {
        Ok(output) => {
            let as_string = String::from_utf8(output.stdout)?;
            let as_unstructured: serde_json::Value = serde_json::from_str(&as_string)?;
            let as_unstructured_arr = as_unstructured.as_array().ok_or("failed to parse JSON as array")?;
            let ns_names: Vec::<String> = as_unstructured_arr.into_iter().map(|ns_json| {
                let ns: Netns = serde_json::from_value(ns_json.clone())?;
                Ok(ns.name)
            })
            .filter_map(|name: Result<String, Box::<dyn std::error::Error>>| {
                match name {
                    Ok(n) => Some(n),
                    Err(err) => {
                        eprintln!("ip_netns_ls: {}", err);
                        None
                    },
                }
            })
            .collect();

            ns_names
        },

        Err(error) => { return Err(Box::new(error)); }
    };

    
    Ok(result)
}
