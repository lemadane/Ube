// Talas Standard Library - Streaming & Binary I/O Package

class Bytes {
    var int capacity
    var int length

    init(int capacity) {
        this._capacity = capacity
        this._length = 0
    }
}

class InputStream {
    int read(Bytes buffer, int offset, int count) {
        return talas_stream_read(this, buffer, offset, count)
    }

    int pipeTo(OutputStream destination) {
        return talas_stream_pipe_to(this, destination)
    }

    void close() {
        talas_stream_close(this)
    }
}

class OutputStream {
    int write(Bytes buffer, int offset, int count) {
        return talas_stream_write(this, buffer, offset, count)
    }

    void close() {
        talas_stream_close(this)
    }
}

class File {
    static InputStream openRead(str path) {
        return talas_file_open_read(path)
    }

    static OutputStream openWrite(str path) {
        return talas_file_open_write(path)
    }

    static str sha256(str path) {
        return talas_file_sha256(path)
    }
}
