# IT9910HD FUSE FS

FUSE File System driver for IT9910HD HDMI MPEG4 (H.264) capture device.

## Setup USB permissions

1. Create or open `/etc/udev/rules.d/50-myusb.rules` file.
2. Add new line:
```
SUBSYSTEMS=="usb", ATTRS{idVendor}=="048d", ATTRS{idProduct}=="9910", GROUP="users", MODE="0666"
```
3. Reload udev rules:
```
sudo udevadm control --reload
```
