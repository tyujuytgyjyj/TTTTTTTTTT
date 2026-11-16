cmake_minimum_required(VERSION 3.16)
project(ogre_bridge)

set(CMAKE_CXX_STANDARD 17)

set(OGRE_SDK "C:/OgreSDK")
set(OGRE_SRC "C:/biqfr/build_ogre_scripts-v3-0/Ogre/ogre-next")
set(OGRE_BUILD "C:/biqfr/build_ogre_scripts-v3-0/Ogre/ogre-next/build")

add_library(ogre_bridge SHARED ogre_bridge.cpp ogre_bridge.h)

target_include_directories(ogre_bridge PRIVATE
    "${OGRE_SDK}/include/OGRE"
    "${OGRE_SDK}/include/OGRE/Hlms/Common"
    "${OGRE_SDK}/include/OGRE/Hlms/Pbs"
    "${OGRE_SDK}/include/OGRE/Hlms/Unlit"
    "${OGRE_SRC}/OgreMain/include"
    "${OGRE_SRC}/Components/Hlms/Common/include"
    "${OGRE_SRC}/Components/Hlms/Pbs/include"
    "${OGRE_SRC}/Components/Hlms/Unlit/include"
    "${OGRE_BUILD}/include"
)

target_link_directories(ogre_bridge PRIVATE
    "${OGRE_SDK}/lib"
    "${OGRE_SDK}/lib/Release"
    "${OGRE_BUILD}/lib/Release"
)

target_link_libraries(ogre_bridge PRIVATE OgreMain OgreHlmsPbs OgreHlmsUnlit user32)


