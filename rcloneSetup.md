### **Prerequisites**
1. **Install rclone** on the new device.
   - On Ubuntu/Debian:
     ```bash
     sudo apt install rclone
     ```
   - For other platforms, download and install rclone from the [official website](https://rclone.org/downloads/).

2. **Have your `rclone.conf` file** ready for the configuration.

---

### **Steps to Set Up Rclone**

#### **1. Place the Configuration File**
1. Copy the `rclone.conf` file to the new device.
2. Move the file to the rclone configuration directory:
   ```bash
   mkdir -p ~/.config/rclone
   mv /path/to/rclone.conf ~/.config/rclone/rclone.conf
   ```

#### **2. Verify the Configuration**
To confirm that the remote is set up correctly, list all remotes:
```bash
rclone listremotes
```
You should see:
```plaintext
RemoteDrive:
```

---

### **Steps to Mount the RustySharing Folder**

#### **1. Create the Mount Directory**
Create a local directory to mount the folder:
```bash
mkdir -p ~/MyDrive
```

#### **2. Mount the Folder**
Run the following command to mount `RustySharing`:
```bash
rclone mount RemoteDrive:"RustySharing" ~/MyDrive --vfs-cache-mode writes
```

- **Explanation**:
  - `RemoteDrive`: Name of the configured remote.
  - `"RustySharing"`: Name of the folder in Google Drive.
  - `~/MyDrive`: Local directory where the folder will be mounted.

#### **3. Verify the Mount**
Navigate to the mount directory and list the contents:
```bash
cd ~/MyDrive
ls
```

You should see the contents of the `RustySharing` folder.

---

### **Unmounting the Drive**
When you’re done, unmount the folder:
```bash
fusermount -u ~/MyDrive
```

---
