fn main() {
    println!("cargo:rustc-link-search=native=C:/OgreSDK/ogre_bridge/build/Release");
    println!("cargo:rustc-link-lib=dylib=ogre_bridge");
}
