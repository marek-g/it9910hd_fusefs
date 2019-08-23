# IT9910HD protocol

IT9910HD is USB 2.0 HDMI MPEG4 (H.264) capture card.

Vendor ID: 0x048d
Product ID: 0x9910

## USB Endpoints

All endpoints are Bulk, NoSync, Data.

* 0x81 EP 1 In - read command responses from here
* 0x02 EP 2 Out - send command requests here
* 0x83 EP 3 In - read data from here

## Request format

1. Header:

    * U32 - request buffer size in bytes
    * U32 - command ID
    * U32 - sub-command ID
    * U32 - request counter (0x99100000 | counter), counter starts from 0

2. Command specific data.

## Response format

1. Header:

    * U32 - response buffer size in bytes
    * U32 - command ID
    * U32 - sub-command ID
    * I32 - result code (>= 0 - OK)

2. Command specific data.

## Requests

### 0x9910F001,1 - DebugQueryTime

Returns device time in [ms].

Request data:

* U32 = input [ms]

Response data:

* U32 = output [ms]

### 0x99100003,1 - GetSource

Returns current audio and video source.

Request data:

* U32 = 0
* U32 = 0

Response data:

* U32 = audio_source (2 for HDMI)
* U32 = video_source (4 for HDMI1)

### 0x99100003,2 - SetSource

Sets the audio and video sources.

Request data:

* U32 = audio_source (2 for HDMI)
* U32 = video_source (4 for HDMI1)

Response data:

* U32 = audio_source (2 for HDMI)
* U32 = video_source (4 for HDMI1)

### 0x9910E001,1 - GetPCGrabber

* U32 = 0x38384001 (command id = capture mode)
* U32 = 0 / unused
* U32 = 0 / unused

Returns:

* U32 - unknown
* U32 - unknown
* U32 - 0 - not in capture mode, 1 - in capture mode

### 0x9910E001,2 - SetPCGrabber

a)

Starts or stops capture mode.

* U32 = 0x38384001 (command id = capture mode)
* U32 = 0 / unused
* U32 = 1 - start, 0 - stop

Note. After starting PCGrabber, wait until GetPCGrabber returns 1.

b)

Set capture format.

Call it 35 times for every index:

* U32 = 0x38382008 (command id = set capture format)
* U32 = 0
* U32 = 4 for Device Model 2 or 5 for other devices
* U32 = index (call it for every index 0..34)
* U32 = 15
* U32 = width = (720 | 1280 | 1920)
* U32 = height = (480 | 576 | 720 | 1080)
* U32 = kbitrate = 2000..20000
* U32 = 0
* U32 = 0
* U32 = framerate (30)
* U32 = 0
* U32 = 0
* U32 = 0
* U32 = 0

### 0x9910F002,1 - GetHWGrabber

Checks firmware version.

Request:

* U32 = 8 (command id = get hw info)
* U8[138] = 0 / unused

Response:

* U8[512]

* bytes[8-14] = [ 0x10 0x11 0x12 0x13 0x14 0x15 0x16 ]
* byte[15] = 0x17 = UHD Device (Device Model 0), 0x27 = Big Device (Device Model 1), 0x37 - ZhanDou device (Device Model 2)

### 0x9910F002,2 - SetHWGrabber

### 0x99100002,2 - SetState

Request data:

* U32 = state (0 = stop, 1 = pause, 2 = start / start after pause)

Response data:

* U32 = state

## Sample program flow

To start capturing:

1. SetPCGrabber(Start = 1) - set device to pc mode.
2. Wait until GetPCGrabber() returns 1.
3. device_model = GetHWGrabber() - get device model.
4. SetSource(2, 4) for device model 2 and HDMI1.
5. for i in 0..35 SetPCGrabber(i, params) - set capture format of your choice.
6. SetState(2) - to start data transfer.

Now the data will be available from EP 3.

If you are using synchronous bulk transfer (unrecommended) it only works to issue other commands after reading whole blocks of data. Then you can decide if you want to read another block or to issue other command. For example, you can read buffers of 16384 bytes (or other size) until you see shorter response. That means it is end of the block.

But be warned that waiting too long to issue other command may also fail.

That's why asynchronous mode is better - you can issue other commands any time you want while reading data at the same time without conflicts.

To stop capturing:

1. SetState(0).
2. SetPCGrabber(Start = 0).
