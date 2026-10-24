#[repr(C)]
pub struct UbeGeoPoint {
    pub latitude: f64,
    pub longitude: f64,
}

#[repr(C)]
pub struct UbeDistance {
    pub meters: f64,
}

#[repr(C)]
pub struct UbeGeoBounds {
    pub southwest: *mut UbeGeoPoint,
    pub northeast: *mut UbeGeoPoint,
}

#[no_mangle]
pub extern "C" fn ube_geopoint_create(lat: f64, lng: f64) -> *mut UbeGeoPoint {
    let p = Box::new(UbeGeoPoint {
        latitude: lat,
        longitude: lng,
    });
    Box::into_raw(p)
}

#[no_mangle]
pub extern "C" fn ube_geopoint_latitude(ptr: *mut UbeGeoPoint) -> f64 {
    if !ptr.is_null() {
        unsafe { (*ptr).latitude }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn ube_geopoint_longitude(ptr: *mut UbeGeoPoint) -> f64 {
    if !ptr.is_null() {
        unsafe { (*ptr).longitude }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn ube_geopoint_distance_to(
    p1: *mut UbeGeoPoint,
    p2: *mut UbeGeoPoint,
) -> *mut UbeDistance {
    if !p1.is_null() && !p2.is_null() {
        unsafe {
            let lat1 = (*p1).latitude.to_radians();
            let lon1 = (*p1).longitude.to_radians();
            let lat2 = (*p2).latitude.to_radians();
            let lon2 = (*p2).longitude.to_radians();

            let dlat = lat2 - lat1;
            let dlon = lon2 - lon1;

            let a =
                (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
            let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

            let earth_radius_meters = 6_371_000.0;
            let distance_m = earth_radius_meters * c;
            ube_distance_create_meters(distance_m)
        }
    } else {
        ube_distance_create_meters(0.0)
    }
}

#[no_mangle]
pub extern "C" fn ube_geopoint_bearing_to(p1: *mut UbeGeoPoint, p2: *mut UbeGeoPoint) -> f64 {
    if !p1.is_null() && !p2.is_null() {
        unsafe {
            let lat1 = (*p1).latitude.to_radians();
            let lon1 = (*p1).longitude.to_radians();
            let lat2 = (*p2).latitude.to_radians();
            let lon2 = (*p2).longitude.to_radians();

            let dlon = lon2 - lon1;
            let y = dlon.sin() * lat2.cos();
            let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
            let initial_bearing_rad = y.atan2(x);
            (initial_bearing_rad.to_degrees() + 360.0) % 360.0
        }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn ube_distance_create_meters(meters: f64) -> *mut UbeDistance {
    let d = Box::new(UbeDistance { meters });
    Box::into_raw(d)
}

#[no_mangle]
pub extern "C" fn ube_distance_create_km(km: f64) -> *mut UbeDistance {
    ube_distance_create_meters(km * 1000.0)
}

#[no_mangle]
pub extern "C" fn ube_distance_create_miles(miles: f64) -> *mut UbeDistance {
    ube_distance_create_meters(miles * 1609.344)
}

#[no_mangle]
pub extern "C" fn ube_distance_meters(ptr: *mut UbeDistance) -> f64 {
    if !ptr.is_null() {
        unsafe { (*ptr).meters }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn ube_distance_km(ptr: *mut UbeDistance) -> f64 {
    if !ptr.is_null() {
        unsafe { (*ptr).meters / 1000.0 }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn ube_distance_miles(ptr: *mut UbeDistance) -> f64 {
    if !ptr.is_null() {
        unsafe { (*ptr).meters / 1609.344 }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn ube_geobounds_create(
    sw: *mut UbeGeoPoint,
    ne: *mut UbeGeoPoint,
) -> *mut UbeGeoBounds {
    let bounds = Box::new(UbeGeoBounds {
        southwest: sw,
        northeast: ne,
    });
    Box::into_raw(bounds)
}

#[no_mangle]
pub extern "C" fn ube_geobounds_contains(b: *mut UbeGeoBounds, p: *mut UbeGeoPoint) -> bool {
    if !b.is_null() && !p.is_null() {
        unsafe {
            let lat = (*p).latitude;
            let lng = (*p).longitude;

            let sw_lat = (*(*b).southwest).latitude;
            let sw_lng = (*(*b).southwest).longitude;
            let ne_lat = (*(*b).northeast).latitude;
            let ne_lng = (*(*b).northeast).longitude;

            lat >= sw_lat && lat <= ne_lat && lng >= sw_lng && lng <= ne_lng
        }
    } else {
        false
    }
}

#[no_mangle]
pub extern "C" fn ube_geolocation_current() -> *mut UbeGeoPoint {
    ube_geopoint_create(14.5995, 120.9842)
}
