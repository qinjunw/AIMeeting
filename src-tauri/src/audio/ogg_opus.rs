use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use ogg::{PacketWriteEndInfo, PacketWriter};
use opus_rs::{Application, OpusEncoder};
use thiserror::Error;

const OPUS_CLOCK_RATE: u32 = 48_000;
const OPUS_FRAME_SAMPLES: usize = 960;
const OPUS_PRE_SKIP: u16 = 312;
const MAX_OPUS_PACKET_BYTES: usize = 1_276;

#[derive(Clone, Debug)]
pub struct OggOpusConfig {
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate_bps: i32,
    pub packets_per_page: u32,
    pub pre_skip: u16,
    pub vendor: String,
}

impl Default for OggOpusConfig {
    fn default() -> Self {
        Self {
            sample_rate: OPUS_CLOCK_RATE,
            channels: 1,
            bitrate_bps: 32_000,
            packets_per_page: 50,
            pre_skip: OPUS_PRE_SKIP,
            vendor: "AIMeeting opus-rs".to_string(),
        }
    }
}

pub struct OggOpusWriter {
    writer: PacketWriter<'static, File>,
    config: OggOpusConfig,
    active_run: Option<ActiveRun>,
    used_serials: HashSet<u32>,
}

impl OggOpusWriter {
    pub fn create(path: &Path, config: OggOpusConfig) -> Result<Self, OggOpusError> {
        validate_config(&config)?;
        let file = File::create(path)?;
        Ok(Self {
            writer: PacketWriter::new(file),
            config,
            active_run: None,
            used_serials: HashSet::new(),
        })
    }

    pub fn begin_run(&mut self, serial: u32) -> Result<(), OggOpusError> {
        if self.active_run.is_some() {
            return Err(OggOpusError::RunAlreadyActive);
        }
        if !self.used_serials.insert(serial) {
            return Err(OggOpusError::DuplicateStreamSerial(serial));
        }

        let mut encoder = OpusEncoder::new(
            self.config.sample_rate as i32,
            self.config.channels as usize,
            Application::Audio,
        )
        .map_err(OggOpusError::Codec)?;
        encoder.bitrate_bps = self.config.bitrate_bps;

        self.writer.write_packet(
            opus_head(&self.config),
            serial,
            PacketWriteEndInfo::EndPage,
            0,
        )?;
        self.writer.write_packet(
            opus_tags(&self.config.vendor),
            serial,
            PacketWriteEndInfo::EndPage,
            0,
        )?;
        self.writer.inner_mut().sync_data()?;

        self.active_run = Some(ActiveRun {
            serial,
            encoder,
            pre_skip: self.config.pre_skip,
            pcm_buffer: Vec::with_capacity(OPUS_FRAME_SAMPLES * 2),
            input_samples: 0,
            encoded_samples: 0,
            packet_count: 0,
            pending_packet: None,
        });
        Ok(())
    }

    pub fn write_pcm(&mut self, samples: &[f32]) -> Result<(), OggOpusError> {
        let active = self.active_run.as_mut().ok_or(OggOpusError::NoActiveRun)?;
        active.input_samples += samples.len() as u64;
        active.pcm_buffer.extend_from_slice(samples);

        let mut packets_to_flush = Vec::new();
        let mut consumed = 0;
        while active.pcm_buffer.len() - consumed >= OPUS_FRAME_SAMPLES {
            let end = consumed + OPUS_FRAME_SAMPLES;
            let frame = active.pcm_buffer[consumed..end].to_vec();
            let packet = active.encode_frame(&frame, None)?;
            if let Some(previous) = active.pending_packet.replace(packet) {
                packets_to_flush.push(previous);
            }
            consumed = end;
        }
        if consumed > 0 {
            active.pcm_buffer.drain(..consumed);
        }

        let serial = active.serial;
        for packet in packets_to_flush {
            self.write_audio_packet(serial, packet, false)?;
        }
        Ok(())
    }

    pub fn finish_run(&mut self) -> Result<RunSummary, OggOpusError> {
        let mut active = self.active_run.take().ok_or(OggOpusError::NoActiveRun)?;

        let mut packets_to_flush = Vec::new();
        if !active.pcm_buffer.is_empty() || active.pending_packet.is_none() {
            let actual_end_granule = self.config.pre_skip as u64 + active.input_samples;
            let mut final_frame = std::mem::take(&mut active.pcm_buffer);
            final_frame.resize(OPUS_FRAME_SAMPLES, 0.0);
            let packet = active.encode_frame(&final_frame, Some(actual_end_granule))?;
            if let Some(previous) = active.pending_packet.replace(packet) {
                packets_to_flush.push(previous);
            }
        }

        for packet in packets_to_flush {
            self.write_audio_packet(active.serial, packet, false)?;
        }
        let final_packet = active
            .pending_packet
            .take()
            .ok_or(OggOpusError::MissingFinalPacket)?;
        self.write_audio_packet(active.serial, final_packet, true)?;
        self.writer.inner_mut().sync_data()?;

        Ok(RunSummary {
            serial: active.serial,
            input_samples: active.input_samples,
            packet_count: active.packet_count,
        })
    }

    pub fn finalize(mut self) -> Result<(), OggOpusError> {
        if self.active_run.is_some() {
            return Err(OggOpusError::RunStillActive);
        }
        self.writer.inner_mut().flush()?;
        self.writer.inner_mut().sync_data()?;
        Ok(())
    }

    fn write_audio_packet(
        &mut self,
        serial: u32,
        packet: EncodedPacket,
        is_final: bool,
    ) -> Result<(), OggOpusError> {
        let end_info = if is_final {
            PacketWriteEndInfo::EndStream
        } else if packet
            .ordinal
            .is_multiple_of(self.config.packets_per_page as u64)
        {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        self.writer
            .write_packet(packet.data, serial, end_info, packet.granule_position)?;
        if end_info != PacketWriteEndInfo::NormalPacket {
            self.writer.inner_mut().sync_data()?;
        }
        Ok(())
    }
}

struct ActiveRun {
    serial: u32,
    encoder: OpusEncoder,
    pre_skip: u16,
    pcm_buffer: Vec<f32>,
    input_samples: u64,
    encoded_samples: u64,
    packet_count: u64,
    pending_packet: Option<EncodedPacket>,
}

impl ActiveRun {
    fn encode_frame(
        &mut self,
        frame: &[f32],
        final_granule: Option<u64>,
    ) -> Result<EncodedPacket, OggOpusError> {
        let mut output = vec![0_u8; MAX_OPUS_PACKET_BYTES];
        let encoded_bytes = self
            .encoder
            .encode(frame, OPUS_FRAME_SAMPLES, &mut output)
            .map_err(OggOpusError::Codec)?;
        output.truncate(encoded_bytes);
        self.encoded_samples += OPUS_FRAME_SAMPLES as u64;
        self.packet_count += 1;

        Ok(EncodedPacket {
            data: output,
            granule_position: final_granule.unwrap_or(self.pre_skip as u64 + self.encoded_samples),
            ordinal: self.packet_count,
        })
    }
}

struct EncodedPacket {
    data: Vec<u8>,
    granule_position: u64,
    ordinal: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunSummary {
    pub serial: u32,
    pub input_samples: u64,
    pub packet_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OggScan {
    pub complete_len: u64,
    pub pages: Vec<OggPageSummary>,
    pub streams: Vec<OggStreamSummary>,
}

impl OggScan {
    pub fn total_duration_samples(&self) -> u64 {
        self.streams
            .iter()
            .map(OggStreamSummary::duration_samples)
            .sum()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OggPageSummary {
    pub offset: u64,
    pub length: u64,
    pub serial: u32,
    pub sequence: u32,
    pub granule_position: u64,
    pub is_bos: bool,
    pub is_eos: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OggStreamSummary {
    pub serial: u32,
    pub has_bos: bool,
    pub has_eos: bool,
    pub pre_skip: u16,
    pub last_granule_position: u64,
}

impl OggStreamSummary {
    pub fn duration_samples(&self) -> u64 {
        self.last_granule_position
            .saturating_sub(self.pre_skip as u64)
    }
}

pub fn scan_ogg_file(path: &Path) -> Result<OggScan, OggOpusError> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut complete_len = 0_u64;
    let mut pages = Vec::new();
    let mut streams = Vec::<OggStreamSummary>::new();

    loop {
        let offset = complete_len;
        let mut header = [0_u8; 27];
        if !read_complete_or_eof(&mut reader, &mut header)? {
            break;
        }
        if &header[0..4] != b"OggS" || header[4] != 0 {
            break;
        }

        let segment_count = header[26] as usize;
        let mut lacing = vec![0_u8; segment_count];
        if !read_complete_or_eof(&mut reader, &mut lacing)? {
            break;
        }
        let body_len = lacing.iter().map(|value| *value as usize).sum::<usize>();
        let mut body = vec![0_u8; body_len];
        if !read_complete_or_eof(&mut reader, &mut body)? {
            break;
        }

        let mut page_bytes = Vec::with_capacity(header.len() + lacing.len() + body.len());
        page_bytes.extend_from_slice(&header);
        page_bytes.extend_from_slice(&lacing);
        page_bytes.extend_from_slice(&body);
        let expected_crc = u32::from_le_bytes(header[22..26].try_into().expect("CRC slice"));
        page_bytes[22..26].fill(0);
        if ogg_crc(&page_bytes) != expected_crc {
            break;
        }

        let serial = u32::from_le_bytes(header[14..18].try_into().expect("serial slice"));
        let granule_position = u64::from_le_bytes(header[6..14].try_into().expect("granule slice"));
        let flags = header[5];
        let length = page_bytes.len() as u64;
        pages.push(OggPageSummary {
            offset,
            length,
            serial,
            sequence: u32::from_le_bytes(header[18..22].try_into().expect("sequence slice")),
            granule_position,
            is_bos: flags & 0x02 != 0,
            is_eos: flags & 0x04 != 0,
        });
        complete_len += length;

        let stream = match streams.iter_mut().find(|stream| stream.serial == serial) {
            Some(stream) => stream,
            None => {
                streams.push(OggStreamSummary {
                    serial,
                    has_bos: false,
                    has_eos: false,
                    pre_skip: 0,
                    last_granule_position: 0,
                });
                streams.last_mut().expect("stream was inserted")
            }
        };
        stream.has_bos |= flags & 0x02 != 0;
        stream.has_eos |= flags & 0x04 != 0;
        if granule_position != u64::MAX {
            stream.last_granule_position = granule_position;
        }
        if body.starts_with(b"OpusHead") && body.len() >= 12 {
            stream.pre_skip = u16::from_le_bytes([body[10], body[11]]);
        }
    }

    Ok(OggScan {
        complete_len,
        pages,
        streams,
    })
}

pub fn recover_truncated_file(path: &Path) -> Result<OggScan, OggOpusError> {
    let scan = scan_ogg_file(path)?;
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.set_len(scan.complete_len)?;

    if let Some(last_page) = scan.pages.last().filter(|page| !page.is_eos) {
        let mut page = vec![0_u8; last_page.length as usize];
        file.seek(SeekFrom::Start(last_page.offset))?;
        file.read_exact(&mut page)?;
        page[5] |= 0x04;
        page[22..26].fill(0);
        let crc = ogg_crc(&page);
        page[22..26].copy_from_slice(&crc.to_le_bytes());
        file.seek(SeekFrom::Start(last_page.offset))?;
        file.write_all(&page)?;
    }
    file.sync_data()?;
    drop(file);

    scan_ogg_file(path)
}

fn validate_config(config: &OggOpusConfig) -> Result<(), OggOpusError> {
    if config.sample_rate != OPUS_CLOCK_RATE || config.channels != 1 {
        return Err(OggOpusError::UnsupportedFormat);
    }
    if config.bitrate_bps <= 0 || config.packets_per_page == 0 {
        return Err(OggOpusError::InvalidConfiguration);
    }
    Ok(())
}

fn opus_head(config: &OggOpusConfig) -> Vec<u8> {
    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.push(1);
    head.push(config.channels);
    head.extend_from_slice(&config.pre_skip.to_le_bytes());
    head.extend_from_slice(&config.sample_rate.to_le_bytes());
    head.extend_from_slice(&0_i16.to_le_bytes());
    head.push(0);
    head
}

fn opus_tags(vendor: &str) -> Vec<u8> {
    let vendor = vendor.as_bytes();
    let mut tags = Vec::with_capacity(16 + vendor.len());
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0_u32.to_le_bytes());
    tags
}

fn read_complete_or_eof(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<bool> {
    match reader.read_exact(buffer) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(error),
    }
}

fn ogg_crc(bytes: &[u8]) -> u32 {
    const POLYNOMIAL: u32 = 0x04C1_1DB7;
    let mut crc = 0_u32;
    for &byte in bytes {
        crc ^= (byte as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ POLYNOMIAL
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[derive(Debug, Error)]
pub enum OggOpusError {
    #[error("an Ogg Opus run is already active")]
    RunAlreadyActive,
    #[error("no Ogg Opus run is active")]
    NoActiveRun,
    #[error("stream serial {0} has already been used")]
    DuplicateStreamSerial(u32),
    #[error("only 48 kHz mono recording is supported")]
    UnsupportedFormat,
    #[error("bitrate and packets per page must be greater than zero")]
    InvalidConfiguration,
    #[error("active run has no final packet")]
    MissingFinalPacket,
    #[error("the active run must be finished before finalizing the file")]
    RunStillActive,
    #[error("Opus codec error: {0}")]
    Codec(&'static str),
    #[error(transparent)]
    Io(#[from] io::Error),
}
