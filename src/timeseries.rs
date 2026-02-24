//! This module defines a `TimeSeries` struct for storing and managing time series data.
//!
//! A `TimeSeries` object maintains two parallel vectors: one for timestamps and one for values.
//! It provides methods to create a new time series and add data points while keeping the series ordered by timestamp.
//!

use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LatLong {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum MetricValue {
    Float(f64),
    Int(i64),
    Location(LatLong),
}

impl std::fmt::Display for MetricValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetricValue::Float(val) => write!(f, "{}", val),
            MetricValue::Int(val) => write!(f, "{}", val),
            MetricValue::Location(loc) => write!(f, "({}, {})", loc.latitude, loc.longitude),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TimeSeriesModel {
    pub device_id: String,
    pub metric: String,
    pub data: Vec<(u64, Value)>,
}

impl MetricValue {
    pub fn as_timeseries(&self, timestamp: u64) -> MetricTimeSeries {
        let mut ts = MetricTimeSeries::new();
        ts.add_point(timestamp, self.clone());
        ts
    }

    pub fn into_float(self) -> Option<f64> {
        match self {
            MetricValue::Float(f) => Some(f),
            MetricValue::Int(i) => Some(i as f64),
            MetricValue::Location(_) => None,
        }
    }

    pub fn into_int(self) -> Option<i64> {
        match self {
            MetricValue::Float(f) => Some(f as i64),
            MetricValue::Int(i) => Some(i),
            MetricValue::Location(_) => None,
        }
    }

    pub fn into_location(self) -> Option<LatLong> {
        match self {
            MetricValue::Location(loc) => Some(loc),
            _ => None,
        }
    }
}

impl From<MetricValue> for serde_json::Value {
    fn from(value: MetricValue) -> Self {
        match value {
            MetricValue::Float(f) => serde_json::Value::Number(
                serde_json::Number::from_f64(f).unwrap_or_else(|| serde_json::Number::from(0)),
            ),
            MetricValue::Int(i) => serde_json::json!(i),
            MetricValue::Location(loc) => serde_json::json!({
                "lat": loc.latitude,
                "long": loc.longitude
            }),
        }
    }
}

// Add trait for different timeseries types
pub trait TimeSeriesConversions: Send + Sync {
    fn to_binary(&self) -> Result<Vec<u8>, TimeseriesSerializationError>;
    fn to_model(&self, device_id: &str, metric: &str) -> TimeSeriesModel;
    fn from_binary(data: &[u8]) -> Result<Self, TimeseriesSerializationError>
    where
        Self: Sized;
    // fn as_float(&self) -> Option<&FloatTimeSeries> {
    //     None
    // }
    // fn as_int(&self) -> Option<&IntTimeSeries> {
    //     None
    // }
    // fn as_location(&self) -> Option<&LocationTimeSeries> {
    //     None
    // }
    // fn as_metric(&self) -> Option<&MetricTimeSeries> {
    //     None
    // }
}

impl LatLong {
    pub fn new(latitude: f64, longitude: f64) -> Self {
        LatLong {
            latitude,
            longitude,
        }
    }
}

pub struct TimeSeriesIter<'a, T> {
    timestamps: std::slice::Iter<'a, u64>,
    values: std::slice::Iter<'a, T>,
}

pub struct TimeSeriesRangeIter<'a, T> {
    series: &'a TimeSeries<T>,
    current_idx: usize,
    end_idx: usize,
}

pub struct TimeSeriesBucketIter<'a, T> {
    series: &'a TimeSeries<T>,
    current_idx: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TimeSeries<T> {
    timestamps: Vec<u64>, // Vector of Unix timestamps in seconds
    values: Vec<T>,       // Vector of values
}

impl<T> TimeSeries<T> {
    /// Creates a new, empty TimeSeries.
    pub fn new() -> Self {
        TimeSeries {
            timestamps: Vec::new(),
            values: Vec::new(),
        }
    }

    /// Adds a new data point to the time series, keeping it ordered.
    pub fn add_point(&mut self, timestamp: u64, value: T) {
        // Find the insertion index using binary search
        match self.timestamps.binary_search(&timestamp) {
            Ok(index) => {
                // Timestamp exists - replace value
                self.values[index] = value;
            }
            Err(index) => {
                // New timestamp - insert both
                self.timestamps.insert(index, timestamp);
                self.values.insert(index, value);
            }
        }
    }

    /// Retrieves the latest data point, if available.
    pub fn latest(&self) -> Option<(u64, &T)> {
        if let (Some(&timestamp), Some(value)) = (self.timestamps.last(), self.values.last()) {
            Some((timestamp, value))
        } else {
            None
        }
    }

    /// Finds the value for a specific timestamp, if it exists.
    pub fn get_value_for_timestamp(&self, timestamp: u64) -> Option<&T> {
        if let Ok(index) = self.timestamps.binary_search(&timestamp) {
            Some(&self.values[index])
        } else {
            None
        }
    }

    /// Clears the time series.
    pub fn clear(&mut self) {
        self.timestamps.clear();
        self.values.clear();
    }

    /// Returns an iterator over the timestamp-value pairs in the time series.
    pub fn iter(&self) -> TimeSeriesIter<'_, T> {
        TimeSeriesIter {
            timestamps: self.timestamps.iter(),
            values: self.values.iter(),
        }
    }

    /// Returns an iterator over a range of timestamp-value pairs within the specified time bounds.
    /// Both `start_ts` and `end_ts` timestamps are inclusive in the range.
    ///
    /// # Arguments
    /// * `start_ts` - Start timestamp (inclusive)
    /// * `end_ts` - End timestamp (inclusive)
    ///
    /// # Example
    /// ```
    /// let mut ts = TimeSeries::new();
    /// ts.add_point(1000, 10.0);
    /// ts.add_point(2000, 20.0);
    /// ts.add_point(3000, 30.0);
    ///
    /// // Will iterate over points at timestamps 1000 and 2000
    /// for (ts, value) in ts.range(1000, 2000) {
    ///     println!("At {}: {}", ts, value);
    /// }
    /// ```
    pub fn range(&self, start_ts: u64, end_ts: u64) -> TimeSeriesRangeIter<'_, T> {
        let start_idx = self
            .timestamps
            .binary_search(&start_ts)
            .unwrap_or_else(|i| i);
        let end_idx = self
            .timestamps
            .binary_search(&end_ts)
            .map(|i| i + 1)
            .unwrap_or_else(|i| i);

        TimeSeriesRangeIter {
            series: self,
            current_idx: start_idx,
            end_idx,
        }
    }

    /// Trims the time series by removing all points outside the specified time range.
    /// Both minimum_ts and maximum_ts are inclusive bounds.
    ///
    /// # Arguments
    /// * `minimum_ts` - Minimum timestamp to keep (inclusive)
    /// * `maximum_ts` - Maximum timestamp to keep (inclusive)
    pub fn trim(&mut self, minimum_ts: u64, maximum_ts: u64) {
        let start_idx = self
            .timestamps
            .binary_search(&minimum_ts)
            .unwrap_or_else(|i| i);

        let end_idx = self
            .timestamps
            .binary_search(&maximum_ts)
            .map(|i| i + 1)
            .unwrap_or_else(|i| i);

        // Remove elements after end_idx
        self.timestamps.truncate(end_idx);
        self.values.truncate(end_idx);

        // Remove elements before start_idx
        if start_idx > 0 {
            self.timestamps.drain(0..start_idx);
            self.values.drain(0..start_idx);
        }
    }

    /// Trim the time series to keep only the last `n` points.
    /// If the time series has fewer than `n` points, it will be left unchanged.
    pub fn keep_last(&mut self, n: usize) {
        if self.len() > n {
            let start_idx = self.len() - n;
            self.timestamps.drain(0..start_idx);
            self.values.drain(0..start_idx);
        }
    }

    /// Returns the number of elements in the time series
    pub fn len(&self) -> usize {
        self.timestamps.len()
    }

    /// Returns true if the time series contains no elements
    pub fn is_empty(&self) -> bool {
        self.timestamps.is_empty()
    }

    /// Returns the first timestamp in the time series, if available.
    pub fn first_timestamp(&self) -> Option<u64> {
        self.timestamps.first().copied()
    }

    /// Merges another TimeSeries into this one, maintaining timestamp order
    pub fn merge(&mut self, other: &TimeSeries<T>)
    where
        T: Clone,
    {
        // Extend with other's data
        for (timestamp, value) in other.timestamps.iter().zip(other.values.iter()) {
            self.add_point(*timestamp, value.clone());
        }
    }

    /// Converts a Unix timestamp into a reverse chronological database key.
    /// Keys are formatted to sort newer timestamps before older ones.
    ///
    /// # Key Schema
    /// Format: {rev_year}{rev_month}{rev_day}{rev_hour}
    /// where each component is reversed by subtracting from its maximum value:
    /// - rev_year = 3000 - year (zero-padded to 4 digits)
    /// - rev_month = 12 - month (zero-padded to 2 digits)
    /// - rev_day = 31 - day (zero-padded to 2 digits)
    /// - rev_hour = 23 - hour (zero-padded to 2 digits)
    ///
    /// # Example
    /// ```
    /// // March 15, 2024 at 14:30:00 UTC becomes:
    /// // year: 3000-2024 = 0976
    /// // month: 12-3 = 09
    /// // day: 31-15 = 16
    /// // hour: 23-14 = 09
    /// // Result: "0976091609"
    /// ```
    pub fn ts_to_key(timestamp: u64) -> String {
        // cap timestamp at 32472147600 (2999-01-01 00:00:00 UTC)
        if timestamp > 32472147600 {
            return "0000000000".to_string();
        }

        let datetime: DateTime<Utc> = Utc
            .timestamp_opt(timestamp as i64, 0)
            .single()
            .expect("Invalid timestamp");

        let rev_year = 3000 - datetime.year() as u32;
        let rev_month = 12 - datetime.month();
        let rev_day = 31 - datetime.day();
        let rev_hour = 23 - datetime.hour();

        format!(
            "{:04}{:02}{:02}{:02}",
            rev_year, rev_month, rev_day, rev_hour
        )
    }

    /// Converts a database key back into a Unix timestamp.
    /// This is the inverse operation of `ts_to_key`.
    ///
    /// # Arguments
    /// * `key` - A 10-character string in format "yyyymmddhh" (reversed chronologically)
    ///
    /// # Returns
    /// * `Result<u64, &'static str>` - Unix timestamp or error if key is invalid
    ///
    /// # Example
    /// ```
    /// let key = "0976091609"; // March 15, 2024 14:00 UTC
    /// let ts = TimeSeries::<f64>::key_to_ts(key).unwrap();
    /// assert_eq!(ts, 1710511200);
    /// ```
    pub fn key_to_ts(key: &str) -> Result<u64, &'static str> {
        if key.len() != 10 {
            return Err("Invalid key length");
        }

        let rev_year = u32::from_str_radix(&key[0..4], 10).map_err(|_| "Invalid year format")?;
        let rev_month = u32::from_str_radix(&key[4..6], 10).map_err(|_| "Invalid month format")?;
        let rev_day = u32::from_str_radix(&key[6..8], 10).map_err(|_| "Invalid day format")?;
        let rev_hour = u32::from_str_radix(&key[8..10], 10).map_err(|_| "Invalid hour format")?;

        let year = 3000 - rev_year;
        let month = 12 - rev_month;
        let day = 31 - rev_day;
        let hour = 23 - rev_hour;

        let datetime = Utc
            .with_ymd_and_hms(year as i32, month as u32, day as u32, hour as u32, 0, 0)
            .single()
            .ok_or("Invalid datetime components")?;

        Ok(datetime.timestamp() as u64)
    }
}

impl<T: Clone> TimeSeries<T> {
    /// Returns an iterator that yields hourly buckets of the time series.
    /// Each bucket is a new TimeSeries containing points for one hour (3600 seconds).
    ///
    /// # Example
    /// ```
    /// let mut ts = TimeSeries::new();
    /// ts.add_point(3600, 10.0);
    /// ts.add_point(3601, 20.0);
    /// ts.add_point(7200, 30.0); // Next hour
    ///
    /// for bucket in ts.buckets() {
    ///     println!("Bucket with {} points", bucket.len());
    /// }
    /// ```
    pub fn buckets(&self) -> TimeSeriesBucketIter<'_, T> {
        TimeSeriesBucketIter {
            series: self,
            current_idx: 0,
        }
    }
}

impl<'a, T> Iterator for TimeSeriesIter<'a, T> {
    type Item = (u64, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        match (self.timestamps.next(), self.values.next()) {
            (Some(&ts), Some(val)) => Some((ts, val)),
            _ => None,
        }
    }
}

impl<'a, T> Iterator for TimeSeriesRangeIter<'a, T> {
    type Item = (u64, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx >= self.end_idx {
            return None;
        }

        let timestamp = self.series.timestamps[self.current_idx];
        let value = &self.series.values[self.current_idx];
        self.current_idx += 1;

        Some((timestamp, value))
    }
}

impl<'a, T: Clone> Iterator for TimeSeriesBucketIter<'a, T> {
    type Item = TimeSeries<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx >= self.series.timestamps.len() {
            return None;
        }

        let mut bucket = TimeSeries::new();
        let current_hour = self.series.timestamps[self.current_idx] / 3600;
        let mut idx = self.current_idx;

        // Collect all points in the current hour
        while idx < self.series.timestamps.len() {
            let ts = self.series.timestamps[idx];
            if ts / 3600 != current_hour {
                break;
            }
            bucket.add_point(ts, self.series.values[idx].clone());
            idx += 1;
        }

        self.current_idx = idx;
        Some(bucket)
    }
}

impl<'a, T> IntoIterator for &'a TimeSeries<T> {
    type Item = (u64, &'a T);
    type IntoIter = TimeSeriesIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SerializationFormat {
    Binary,
    Json,
}

#[derive(Debug, Clone, Copy)]
pub enum TimeseriesStorageFormat {
    BinaryFloatSeries,
    BinaryIntSeries,
    BinaryLocationSeries,
    BinaryMetricSeries,
}

#[derive(Error, Debug)]
pub enum TimeseriesSerializationError {
    #[error("Binary serialization error: {0}")]
    BincodeError(#[from] bincode::Error),
    #[error("Unsupported serialization format")]
    UnsupportedFormat,
    #[error("Wrong type byte for deserialization")]
    WrongTypeByte(String),
    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
}

impl<T: Serialize> TimeSeries<T> {
    pub fn serialize(
        &self,
        format: SerializationFormat,
    ) -> Result<Vec<u8>, TimeseriesSerializationError> {
        match format {
            SerializationFormat::Binary => Ok(bincode::serialize(self)?),
            SerializationFormat::Json => Err(TimeseriesSerializationError::UnsupportedFormat),
        }
    }

    pub fn deserialize(
        bytes: &[u8],
        format: SerializationFormat,
    ) -> Result<Self, TimeseriesSerializationError>
    where
        T: for<'de> Deserialize<'de>,
    {
        match format {
            SerializationFormat::Binary => Ok(bincode::deserialize(bytes)?),
            SerializationFormat::Json => Err(TimeseriesSerializationError::UnsupportedFormat),
        }
    }
}

pub type IntTimeSeries = TimeSeries<i64>;

impl TimeSeriesConversions for IntTimeSeries {
    fn to_binary(&self) -> Result<Vec<u8>, TimeseriesSerializationError> {
        // convert the type to a single byte
        let type_byte = TimeseriesStorageFormat::BinaryIntSeries as u8;
        // serialize the type and the data
        // the result is a Vec<u8> with the type byte followed by the serialized data
        let mut data = bincode::serialize(&type_byte)?;
        data.extend(bincode::serialize(self)?);
        Ok(data)
    }

    fn to_model(&self, device_id: &str, metric: &str) -> TimeSeriesModel {
        let data = self
            .iter()
            .map(|(ts, val)| (ts, serde_json::Value::from(val.clone())))
            .collect();
        TimeSeriesModel {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            data,
        }
    }

    fn from_binary(data: &[u8]) -> Result<Self, TimeseriesSerializationError>
    where
        Self: Sized,
    {
        let type_byte = data[0];
        // check if the type is correct
        if type_byte != TimeseriesStorageFormat::BinaryIntSeries as u8 {
            return Err(TimeseriesSerializationError::WrongTypeByte(String::from(
                "Cannot deserialize binary data into IntTimeSeries. Wrong type byte.",
            )));
        }
        // check length
        if data.len() < 2 {
            return Err(TimeseriesSerializationError::WrongTypeByte(String::from(
                "Cannot deserialize binary data into IntTimeSeries. Data too short.",
            )));
        }
        // deserialize the data
        Ok(bincode::deserialize(&data[1..])?)
    }
}

pub type FloatTimeSeries = TimeSeries<f64>;

impl TimeSeriesConversions for FloatTimeSeries {
    fn to_binary(&self) -> Result<Vec<u8>, TimeseriesSerializationError> {
        // convert the type to a single byte
        let type_byte = TimeseriesStorageFormat::BinaryFloatSeries as u8;
        // serialize the type and the data
        // the result is a Vec<u8> with the type byte followed by the serialized data
        let mut data = bincode::serialize(&type_byte)?;
        data.extend(bincode::serialize(self)?);
        Ok(data)
    }

    fn to_model(&self, device_id: &str, metric: &str) -> TimeSeriesModel {
        let data = self
            .iter()
            .map(|(ts, val)| (ts, serde_json::Value::from(val.clone())))
            .collect();
        TimeSeriesModel {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            data,
        }
    }

    fn from_binary(data: &[u8]) -> Result<Self, TimeseriesSerializationError>
    where
        Self: Sized,
    {
        let type_byte = data[0];
        // check if the type is correct
        if type_byte != TimeseriesStorageFormat::BinaryFloatSeries as u8 {
            return Err(TimeseriesSerializationError::WrongTypeByte(String::from(
                "Cannot deserialize binary data into FloatTimeSeries. Wrong type byte.",
            )));
        }
        // check length
        if data.len() < 2 {
            return Err(TimeseriesSerializationError::WrongTypeByte(String::from(
                "Cannot deserialize binary data into FloatTimeSeries. Data too short.",
            )));
        }
        // deserialize the data
        Ok(bincode::deserialize(&data[1..])?)
    }
}

pub type LocationTimeSeries = TimeSeries<LatLong>;

impl TimeSeriesConversions for LocationTimeSeries {
    fn to_binary(&self) -> Result<Vec<u8>, TimeseriesSerializationError> {
        // convert the type to a single byte
        let type_byte = TimeseriesStorageFormat::BinaryLocationSeries as u8;
        // serialize the type and the data
        // the result is a Vec<u8> with the type byte followed by the serialized data
        let mut data = bincode::serialize(&type_byte)?;
        data.extend(bincode::serialize(self)?);
        Ok(data)
    }

    fn to_model(&self, device_id: &str, metric: &str) -> TimeSeriesModel {
        let data = self
            .iter()
            .map(|(ts, val)| {
                (
                    ts,
                    serde_json::Value::from(MetricValue::Location(val.clone())),
                )
            })
            .collect();
        TimeSeriesModel {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            data,
        }
    }

    fn from_binary(data: &[u8]) -> Result<Self, TimeseriesSerializationError>
    where
        Self: Sized,
    {
        let type_byte = data[0];
        // check if the type is correct
        if type_byte != TimeseriesStorageFormat::BinaryLocationSeries as u8 {
            return Err(TimeseriesSerializationError::WrongTypeByte(String::from(
                "Cannot deserialize binary data into LocationTimeSeries. Wrong type byte.",
            )));
        }
        // check length
        if data.len() < 2 {
            return Err(TimeseriesSerializationError::WrongTypeByte(String::from(
                "Cannot deserialize binary data into LocationTimeSeries. Data too short.",
            )));
        }
        // deserialize the data
        Ok(bincode::deserialize(&data[1..])?)
    }
}

pub type MetricTimeSeries = TimeSeries<MetricValue>;

impl MetricTimeSeries {
    pub fn to_float_series(&self) -> Option<FloatTimeSeries> {
        let mut float_ts = FloatTimeSeries::new();
        for (ts, val) in self.iter() {
            if let Some(float_val) = val.clone().into_float() {
                float_ts.add_point(ts, float_val);
            } else {
                return None; // Return None if any value can't be converted
            }
        }
        Some(float_ts)
    }

    pub fn to_int_series(&self) -> Option<IntTimeSeries> {
        let mut int_ts = IntTimeSeries::new();
        for (ts, val) in self.iter() {
            if let Some(int_val) = val.clone().into_int() {
                int_ts.add_point(ts, int_val);
            } else {
                return None;
            }
        }
        Some(int_ts)
    }

    pub fn to_location_series(&self) -> Option<LocationTimeSeries> {
        let mut loc_ts = LocationTimeSeries::new();
        for (ts, val) in self.iter() {
            if let Some(loc_val) = val.clone().into_location() {
                loc_ts.add_point(ts, loc_val);
            } else {
                return None;
            }
        }
        Some(loc_ts)
    }
}

impl From<&FloatTimeSeries> for MetricTimeSeries {
    fn from(float_ts: &FloatTimeSeries) -> Self {
        let mut metric_ts = MetricTimeSeries::new();
        for (ts, val) in float_ts.into_iter() {
            metric_ts.add_point(ts, MetricValue::Float(*val));
        }
        metric_ts
    }
}

impl From<&IntTimeSeries> for MetricTimeSeries {
    fn from(int_ts: &IntTimeSeries) -> Self {
        let mut metric_ts = MetricTimeSeries::new();
        for (ts, val) in int_ts.into_iter() {
            metric_ts.add_point(ts, MetricValue::Int(*val));
        }
        metric_ts
    }
}

impl From<&LocationTimeSeries> for MetricTimeSeries {
    fn from(loc_ts: &LocationTimeSeries) -> Self {
        let mut metric_ts = MetricTimeSeries::new();
        for (ts, val) in loc_ts.into_iter() {
            metric_ts.add_point(ts, MetricValue::Location(val.clone()));
        }
        metric_ts
    }
}

impl TimeSeriesConversions for MetricTimeSeries {
    fn to_binary(&self) -> Result<Vec<u8>, TimeseriesSerializationError> {
        // convert the type to a single byte
        let type_byte = TimeseriesStorageFormat::BinaryMetricSeries as u8;
        // serialize the type and the data
        // the result is a Vec<u8> with the type byte followed by the serialized data
        let mut data = bincode::serialize(&type_byte)?;
        data.extend(bincode::serialize(self)?);
        Ok(data)
    }

    fn to_model(&self, device_id: &str, metric: &str) -> TimeSeriesModel {
        let data = self
            .iter()
            .map(|(ts, val)| (ts, serde_json::Value::from(val.clone())))
            .collect();
        TimeSeriesModel {
            device_id: device_id.to_string(),
            metric: metric.to_string(),
            data,
        }
    }

    fn from_binary(data: &[u8]) -> Result<Self, TimeseriesSerializationError>
    where
        Self: Sized,
    {
        let type_byte = data[0];

        //  first check if the type is metric series
        if type_byte == TimeseriesStorageFormat::BinaryMetricSeries as u8 {
            // this is a native metric series
            if data.len() < 2 {
                return Err(TimeseriesSerializationError::WrongTypeByte(String::from(
                    "Cannot deserialize binary data into MetricTimeSeries. Data too short.",
                )));
            }
            return Ok(bincode::deserialize(&data[1..])?);
        }

        // we can construct a metric time series from any of the other types
        if type_byte == TimeseriesStorageFormat::BinaryFloatSeries as u8 {
            let float_ts = FloatTimeSeries::from_binary(data)?;
            return Ok(MetricTimeSeries::from(&float_ts));
        } else if type_byte == TimeseriesStorageFormat::BinaryIntSeries as u8 {
            let int_ts = IntTimeSeries::from_binary(data)?;
            return Ok(MetricTimeSeries::from(&int_ts));
        } else if type_byte == TimeseriesStorageFormat::BinaryLocationSeries as u8 {
            let loc_ts = LocationTimeSeries::from_binary(data)?;
            return Ok(MetricTimeSeries::from(&loc_ts));
        }

        Err(TimeseriesSerializationError::WrongTypeByte(String::from(
            "Cannot deserialize binary data into MetricTimeSeries. Wrong type byte.",
        )))
    }
}

#[cfg(test)]
mod tests;
